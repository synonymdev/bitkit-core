import java.io.ByteArrayOutputStream
import java.io.File
import java.util.zip.ZipEntry
import java.util.zip.ZipFile
import java.util.zip.ZipOutputStream

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

val validateMinifiedConsumerFieldOrder by tasks.registering {
    group = "verification"
    description = "Asserts R8 full mode keeps JNA Structure.FieldOrder annotations."
    dependsOn("compileReleaseKotlin")

    val consumerRules = layout.projectDirectory.file("consumer-rules.pro")
    inputs.file(consumerRules)

    doLast {
        val classesDir = layout.buildDirectory.dir("tmp/kotlin-classes/release").get().asFile
        if (!classesDir.isDirectory) {
            throw GradleException("Release Kotlin classes missing at '${classesDir.path}'")
        }

        val androidJar = File(
            android.sdkDirectory,
            "platforms/android-${android.compileSdk}/android.jar",
        )
        if (!androidJar.isFile) {
            throw GradleException("android.jar missing at '${androidJar.path}'")
        }

        val jnaAar = configurations.getByName("releaseCompileClasspath").files
            .firstOrNull { it.name.startsWith("jna-") && it.name.endsWith(".aar") }
            ?: throw GradleException("JNA AAR missing from releaseCompileClasspath")

        val work = layout.buildDirectory.dir("r8-fieldorder").get().asFile
        work.deleteRecursively()
        work.mkdirs()
        val jnaJar = work.resolve("jna-classes.jar")
        ZipFile(jnaAar).use { zip ->
            val entry = zip.getEntry("classes.jar")
                ?: throw GradleException("JNA AAR is missing classes.jar: '${jnaAar.path}'")
            zip.getInputStream(entry).use { input ->
                jnaJar.outputStream().use { input.copyTo(it) }
            }
        }

        val seedRules = work.resolve("consumer-seed.pro")
        seedRules.writeText(
            """
            -keep class com.synonym.bitkitcore.UniffiLib { *; }
            -keep class com.synonym.bitkitcore.RustBufferStruct { *; }
            -dontwarn **
            -ignorewarnings
            """.trimIndent(),
        )

        val programJar = work.resolve("program.jar")
        ZipOutputStream(programJar.outputStream()).use { zip ->
            classesDir.walkTopDown().filter { it.isFile }.forEach { file ->
                val name = classesDir.toPath().relativize(file.toPath()).toString().replace('\\', '/')
                zip.putNextEntry(ZipEntry(name))
                file.inputStream().use { it.copyTo(zip) }
                zip.closeEntry()
            }
        }

        val r8Out = work.resolve("minified")
        r8Out.mkdirs()
        val r8 = configurations.detachedConfiguration(
            dependencies.create("com.android.tools:r8:8.5.35"),
        )
        r8.isTransitive = false

        val libJars = mutableListOf(androidJar.absolutePath)
        configurations.getByName("releaseCompileClasspath").files.forEach { file ->
            if (file.name.startsWith("jna-")) {
                return@forEach
            }
            if (file.extension == "jar") {
                libJars.add(file.absolutePath)
            }
            if (file.extension == "aar") {
                ZipFile(file).use { zip ->
                    val entry = zip.getEntry("classes.jar") ?: return@use
                    val extracted = work.resolve("${file.nameWithoutExtension}-classes.jar")
                    zip.getInputStream(entry).use { input ->
                        extracted.outputStream().use { input.copyTo(it) }
                    }
                    libJars.add(extracted.absolutePath)
                }
            }
        }

        javaexec {
            classpath = r8
            mainClass.set("com.android.tools.r8.R8")
            args = buildList {
                add("--release")
                add("--classfile")
                add("--output")
                add(r8Out.resolve("minified.jar").absolutePath)
                libJars.forEach { jar ->
                    add("--lib")
                    add(jar)
                }
                add("--pg-conf")
                add(consumerRules.asFile.absolutePath)
                add("--pg-conf")
                add(seedRules.absolutePath)
                add(programJar.absolutePath)
                add(jnaJar.absolutePath)
            }
        }

        val minifiedJar = r8Out.resolve("minified.jar")
        val javap = ProcessBuilder(
            "javap",
            "-verbose",
            "-classpath",
            minifiedJar.absolutePath,
            "com.synonym.bitkitcore.RustBufferStruct",
        ).redirectErrorStream(true).start()
        val javapOut = javap.inputStream.bufferedReader().readText()
        if (javap.waitFor() != 0) {
            throw GradleException(
                "Minified consumer dropped RustBufferStruct:\n$javapOut",
            )
        }
        if (!javapOut.contains("com.sun.jna.Structure\$FieldOrder")) {
            throw GradleException(
                "Minified consumer dropped @Structure.FieldOrder on RustBufferStruct:\n$javapOut",
            )
        }
    }
}

tasks.matching { it.name == "bundleReleaseAar" || it.name.startsWith("publish") }.configureEach {
    dependsOn(
        validateReleaseNativeLibraries,
        validateConsumerKeepRules,
        validateMinifiedConsumerFieldOrder,
    )
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
