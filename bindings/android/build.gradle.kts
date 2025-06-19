buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:8.1.4")
    }
}

// library version is defined in gradle.properties
val libraryVersion: String by project

group = "com.synonym"
version = libraryVersion
