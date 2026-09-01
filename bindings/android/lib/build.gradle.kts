import java.io.ByteArrayOutputStream
import java.io.File
import java.util.zip.ZipFile

plugins {
    id("com.android.library")
    kotlin("android")
    kotlin("plugin.serialization")

    id("maven-publish")
    id("signing")
    id("org.jlleitschuh.gradle.ktlint") version "11.6.1"
}

repositories {
    mavenCentral()
    google()
}

android {
    namespace = "com.synonym.bitkitcore"
    compileSdk = 34

    defaultConfig {
        minSdk = 21
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }

    kotlinOptions {
        jvmTarget = "1.8"
    }

    buildTypes {
        getByName("release") {
            isMinifyEnabled = false
            proguardFiles(file("proguard-android-optimize.txt"), file("proguard-rules.pro"))
        }
    }

    publishing {
        singleVariant("release") {
            withSourcesJar()
            withJavadocJar()
        }
    }
}

dependencies {
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation("org.jetbrains.kotlin:kotlin-stdlib-jdk8")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.7.3")
    implementation("androidx.appcompat:appcompat:1.6.1")
    implementation("androidx.core:core-ktx:1.12.0")
    implementation("org.jetbrains.kotlinx:atomicfu:0.23.1")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.6.0")
    api("org.slf4j:slf4j-api:1.7.36")
}

val androidNativeAbis = listOf("armeabi-v7a", "arm64-v8a", "x86", "x86_64")

fun executableFromPath(name: String): String? {
    return System.getenv("PATH")
        ?.split(File.pathSeparator)
        ?.asSequence()
        ?.map { File(it, name) }
        ?.firstOrNull { it.canExecute() }
        ?.absolutePath
}

fun findReadelf(): String {
    executableFromPath("llvm-readelf")?.let { return it }
    executableFromPath("readelf")?.let { return it }

    return listOf("ANDROID_NDK_ROOT", "ANDROID_NDK_HOME", "NDK_HOME")
        .mapNotNull { System.getenv(it) }
        .map { File(it, "toolchains/llvm/prebuilt") }
        .firstNotNullOfOrNull { prebuiltDir ->
            if (!prebuiltDir.isDirectory) return@firstNotNullOfOrNull null

            prebuiltDir
                .walkTopDown()
                .firstOrNull { it.name == "llvm-readelf" && it.canExecute() }
                ?.absolutePath
        }
        ?: throw GradleException(
            "llvm-readelf or readelf is required to validate Android native debug symbols"
        )
}

fun Project.runReadelf(readelf: String, vararg args: String): Pair<Int, String> {
    val stdout = ByteArrayOutputStream()
    val stderr = ByteArrayOutputStream()
    val result = exec {
        commandLine(readelf, *args)
        standardOutput = stdout
        errorOutput = stderr
        isIgnoreExitValue = true
    }

    return result.exitValue to stdout.toString().ifBlank { stderr.toString() }
}

fun String.parseElfAlignment(): Long {
    return if (startsWith("0x")) {
        removePrefix("0x").toLong(16)
    } else {
        toLong()
    }
}

val validateReleaseNativeLibraries by tasks.registering {
    group = "verification"
    description = "Validates release JNI libraries are stripped and keep 16 KB LOAD alignment."

    doLast {
        val readelf = findReadelf()
        val loadAlignmentRegex = Regex("""^\s*LOAD\s+.*\s+(0x[0-9a-fA-F]+|\d+)\s*$""")

        androidNativeAbis.forEach { abi ->
            val lib = layout.projectDirectory.file("src/main/jniLibs/$abi/libbitkitcore.so").asFile
            if (!lib.isFile) {
                throw GradleException("Android native library missing at '${lib.path}'")
            }

            val (sectionsExit, sections) = runReadelf(readelf, "-S", lib.absolutePath)
            if (sectionsExit != 0) {
                throw GradleException("Unable to inspect Android native library sections: '${lib.path}'")
            }
            if (Regex("""\.debug_""").containsMatchIn(sections)) {
                throw GradleException("Android release native library still contains .debug_* sections: '${lib.path}'")
            }

            val wideHeaders = runReadelf(readelf, "-W", "-l", lib.absolutePath)
            val headers = if (wideHeaders.first == 0) {
                wideHeaders.second
            } else {
                val fallbackHeaders = runReadelf(readelf, "-l", lib.absolutePath)
                if (fallbackHeaders.first != 0) {
                    throw GradleException("Unable to inspect Android native library headers: '${lib.path}'")
                }
                fallbackHeaders.second
            }

            val alignments = headers
                .lineSequence()
                .mapNotNull { loadAlignmentRegex.matchEntire(it)?.groupValues?.get(1)?.parseElfAlignment() }
                .toList()

            if (alignments.isEmpty() || alignments.any { it < 16_384 }) {
                throw GradleException("Android native library is not 16 KB page-size aligned: '${lib.path}'")
            }
        }
    }
}

val validateConsumerKeepRules by tasks.registering {
    group = "verification"
    description = "Validates Android consumer keep rules exist for R8."

    val consumerRules = layout.projectDirectory.file("consumer-rules.pro")
    inputs.file(consumerRules)

    doLast {
        val file = consumerRules.asFile
        if (!file.isFile || file.readText().isBlank()) {
            throw GradleException("Android consumer keep rules missing at '${file.path}'")
        }
    }
}

val validateReleaseManifest by tasks.registering {
    group = "verification"
    description = "Validates Android release provenance and artifact hashes are present."

    val releaseManifest = rootProject.layout.projectDirectory.file("release-manifest.json")
    inputs.file(releaseManifest)

    doLast {
        val file = releaseManifest.asFile
        if (!file.isFile || file.readText().isBlank()) {
            throw GradleException("Android release manifest missing at '${file.path}'")
        }

        val text = file.readText()
        val requiredKeys = listOf(
            "sourceRevision",
            "gobleyRevision",
            "androidAar",
            "nativeDebugSymbols",
            "armeabi-v7a",
            "arm64-v8a",
            "x86",
            "x86_64",
        )
        val missingKeys = requiredKeys.filterNot { "\"$it\"" in text }
        if (missingKeys.isNotEmpty()) {
            throw GradleException("Android release manifest is missing keys: $missingKeys")
        }

        val hashes = Regex("\"sha256\"\\s*:\\s*\"[0-9a-f]{64}\"").findAll(text).count()
        val nativeHashes = Regex("\"(?:armeabi-v7a|arm64-v8a|x86|x86_64)\"\\s*:\\s*\"[0-9a-f]{64}\"")
            .findAll(text)
            .count()
        if (hashes != 2 || nativeHashes != androidNativeAbis.size) {
            throw GradleException("Android release manifest does not contain every required SHA-256 hash")
        }
    }
}

tasks.matching { it.name == "bundleReleaseAar" || it.name.startsWith("publish") }.configureEach {
    dependsOn(validateReleaseNativeLibraries, validateConsumerKeepRules)
}

tasks.matching { it.name.startsWith("publish") }.configureEach {
    dependsOn(validateReleaseManifest)
}

tasks.matching { it.name == "bundleReleaseAar" }.configureEach {
    doLast {
        val aars = layout.buildDirectory.dir("outputs/aar").get().asFile
            .listFiles()
            ?.filter { it.isFile && it.name.endsWith(".aar") && it.name.contains("release") }
            .orEmpty()

        if (aars.isEmpty()) {
            throw GradleException("Release AAR missing after bundleReleaseAar")
        }

        aars.forEach { aar ->
            ZipFile(aar).use { zip ->
                val entry = zip.getEntry("proguard.txt")
                    ?: throw GradleException(
                        "Release AAR is missing consumer keep rules (proguard.txt): '${aar.path}'"
                    )
                val text = zip.getInputStream(entry).bufferedReader().readText()
                if (text.isBlank()) {
                    throw GradleException(
                        "Release AAR consumer keep rules are empty: '${aar.path}'"
                    )
                }
            }
        }
    }
}

afterEvaluate {
    publishing {
        publications {
            create<MavenPublication>("maven") {
                val mavenArtifactId = "bitkit-core-android"
                groupId = providers.gradleProperty("group").orNull ?: "com.synonym"
                artifactId = mavenArtifactId
                version = providers.gradleProperty("version").orNull ?: "0.0.0"

                from(components["release"])
                artifact(rootProject.layout.projectDirectory.file("native-debug-symbols.zip")) {
                    classifier = "native-debug-symbols"
                    extension = "zip"
                }
                artifact(rootProject.layout.projectDirectory.file("release-manifest.json")) {
                    classifier = "release-manifest"
                    extension = "json"
                }
                pom {
                    name.set(mavenArtifactId)
                    description.set("Bitkit Core Android bindings.")
                    url.set("https://github.com/synonymdev/bitkit-core")
                    licenses {
                        license {
                            name.set("MIT")
                            url.set("https://github.com/synonymdev/bitkit-core/blob/master/LICENSE")
                        }
                    }
                    developers {
                        developer {
                            id.set("synonymdev")
                            name.set("Synonym")
                            email.set("noreply@synonym.to")
                        }
                    }
                }
            }
        }
        repositories {
            maven {
                val repo = System.getenv("GITHUB_REPO")
                    ?: providers.gradleProperty("gpr.repo").orNull
                    ?: "synonymdev/bitkit-core"
                name = "GitHubPackages"
                url = uri("https://maven.pkg.github.com/$repo")
                credentials {
                    username = System.getenv("GITHUB_ACTOR") ?: providers.gradleProperty("gpr.user").orNull
                    password = System.getenv("GITHUB_TOKEN") ?: providers.gradleProperty("gpr.key").orNull
                }
            }
        }
    }
}

ktlint {
    filter {
        exclude { entry ->
            entry.file.toString().contains("main")
        }
    }
}
