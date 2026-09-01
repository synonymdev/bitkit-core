import groovy.json.JsonSlurper
import java.io.ByteArrayOutputStream
import java.io.File
import java.security.MessageDigest
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

fun File.sha256(): String {
    val digest = MessageDigest.getInstance("SHA-256").digest(readBytes())
    return digest.joinToString("") { byte ->
        (byte.toInt() and 0xff).toString(16).padStart(2, '0')
    }
}

fun Map<*, *>.requiredMap(key: String): Map<*, *> {
    return this[key] as? Map<*, *>
        ?: throw GradleException("Android release manifest is missing object '$key'")
}

fun Map<*, *>.requiredString(key: String): String {
    return this[key] as? String
        ?: throw GradleException("Android release manifest is missing string '$key'")
}

fun gitHead(repository: File): String {
    val process = ProcessBuilder("git", "rev-parse", "HEAD")
        .directory(repository)
        .redirectErrorStream(true)
        .start()
    val output = process.inputStream.bufferedReader().readText().trim()
    if (process.waitFor() != 0 || !Regex("[0-9a-f]{40}").matches(output)) {
        throw GradleException("Unable to resolve source revision: $output")
    }
    return output
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
    description = "Validates Android release provenance and artifact hashes."

    dependsOn("bundleReleaseAar")

    val repositoryRoot = rootProject.projectDir.parentFile.parentFile
    val releaseManifest = rootProject.layout.projectDirectory.file("release-manifest.json")
    val releaseAar = layout.buildDirectory.file("outputs/aar/lib-release.aar")
    val nativeDebugSymbols = rootProject.layout.projectDirectory.file("native-debug-symbols.zip")
    val nativeLibraries = androidNativeAbis.map { abi ->
        layout.projectDirectory.file("src/main/jniLibs/$abi/libbitkitcore.so")
    }
    inputs.file(releaseManifest)
    inputs.file(releaseAar)
    inputs.file(nativeDebugSymbols)
    inputs.files(nativeLibraries)

    doLast {
        val file = releaseManifest.asFile
        if (!file.isFile || file.readText().isBlank()) {
            throw GradleException("Android release manifest missing at '${file.path}'")
        }

        val manifest = JsonSlurper().parse(file) as? Map<*, *>
            ?: throw GradleException("Android release manifest root must be a JSON object")
        val expectedVersion = providers.gradleProperty("version").get()
        val expectedGobleyRepository = providers.gradleProperty("gobleyRepository").get()
        val expectedGobleyRevision = providers.gradleProperty("gobleyRevision").get()
        val expectedSourceRevision = gitHead(repositoryRoot)

        if (manifest.requiredString("version") != expectedVersion) {
            throw GradleException("Android release manifest version does not match Gradle version")
        }
        if (manifest.requiredString("sourceRevision") != expectedSourceRevision) {
            throw GradleException("Android release manifest source revision does not match HEAD")
        }
        if (manifest["sourceDirty"] != false) {
            throw GradleException("Android release manifest must describe a clean source tree")
        }
        if (manifest.requiredString("gobleyRepository") != expectedGobleyRepository) {
            throw GradleException("Android release manifest Gobley repository does not match the build pin")
        }
        if (manifest.requiredString("gobleyRevision") != expectedGobleyRevision) {
            throw GradleException("Android release manifest Gobley revision does not match the build pin")
        }

        fun validateArtifact(label: String, details: Map<*, *>, expectedFile: File) {
            val declaredFile = repositoryRoot.resolve(details.requiredString("path")).canonicalFile
            if (declaredFile != expectedFile.canonicalFile) {
                throw GradleException("Android release manifest $label path is incorrect")
            }
            if (!expectedFile.isFile) {
                throw GradleException("Android release artifact missing at '${expectedFile.path}'")
            }
            if (details.requiredString("sha256") != expectedFile.sha256()) {
                throw GradleException("Android release manifest $label SHA-256 does not match the artifact")
            }
        }

        val artifacts = manifest.requiredMap("artifacts")
        validateArtifact("AAR", artifacts.requiredMap("androidAar"), releaseAar.get().asFile)
        validateArtifact(
            "debug symbols",
            artifacts.requiredMap("nativeDebugSymbols"),
            nativeDebugSymbols.asFile
        )

        val expectedNativeHashes = artifacts.requiredMap("nativeLibraries")
        androidNativeAbis.zip(nativeLibraries).forEach { (abi, nativeLibrary) ->
            val library = nativeLibrary.asFile
            if (!library.isFile || expectedNativeHashes.requiredString(abi) != library.sha256()) {
                throw GradleException("Android release manifest $abi SHA-256 does not match the library")
            }
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
