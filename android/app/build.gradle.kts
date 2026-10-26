plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

val gitRev: String = providers.exec { commandLine("git", "rev-parse", "--short=7", "HEAD"); isIgnoreExitValue = true }
    .standardOutput.asText.getOrElse("unknown").trim().ifEmpty { "unknown" }

android {
    namespace = "io.z2labs.sathunter"
    compileSdk = 35
    defaultConfig {
        applicationId = "io.z2labs.sathunter"
        minSdk = 26
        targetSdk = 35
        versionCode = 8
        versionName = "0.2.0-beta7"
        buildConfigField("String", "GIT_REV", "\"$gitRev\"")
        ndk { abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64") }
    }
    buildFeatures {
        compose = true
        buildConfig = true
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    // Release key from CI secrets (ANDROID_KEYSTORE_B64 / ANDROID_KEYSTORE_PASSWORD): every APK
    // gets the same signature, so a new build installs over the old one. Without it (forks,
    // local builds) the debug key is used.
    val releaseKs = System.getenv("ANDROID_KEYSTORE_FILE")?.let { file(it) }?.takeIf { it.exists() }
    signingConfigs {
        if (releaseKs != null) create("release") {
            storeFile = releaseKs
            storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
            keyAlias = System.getenv("ANDROID_KEY_ALIAS") ?: "sathunter"
            keyPassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
        }
    }
    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName(if (releaseKs != null) "release" else "debug")
        }
    }
    packaging { jniLibs { useLegacyPackaging = false } }
}

dependencies {
    val bom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(bom)
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
}
