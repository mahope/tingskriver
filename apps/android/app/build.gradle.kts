plugins {
    id("com.android.application")
}

android {
    namespace = "dk.tingskriver.android"
    compileSdk = 36

    defaultConfig {
        applicationId = "dk.tingskriver.android"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0-dev.1"
    }

    val releaseKeystore = System.getenv("TINGSKRIVER_ANDROID_KEYSTORE")
    val releaseStorePassword = System.getenv("TINGSKRIVER_ANDROID_KEYSTORE_PASSWORD")
    val releaseKeyAlias = System.getenv("TINGSKRIVER_ANDROID_KEY_ALIAS")
    val releaseKeyPassword = System.getenv("TINGSKRIVER_ANDROID_KEY_PASSWORD")
    if (listOf(releaseKeystore, releaseStorePassword, releaseKeyAlias, releaseKeyPassword)
            .all { !it.isNullOrBlank() }) {
        val releaseSigning = signingConfigs.create("release") {
            storeFile = file(releaseKeystore!!)
            storePassword = releaseStorePassword
            keyAlias = releaseKeyAlias
            keyPassword = releaseKeyPassword
        }
        buildTypes.getByName("release") {
            signingConfig = releaseSigning
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
}
