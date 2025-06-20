buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:8.5.2")
    }
}

// library version is defined in gradle.properties
val libraryVersion: String by project

group = "com.synonym"
version = libraryVersion
