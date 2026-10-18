plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

/**
 * The version from the release tag, as on iOS (scripts/resolve-release-version.sh): RELEASE_VERSION
 * (CI passes the tag), else `git describe`, else 0.0.0-dev. versionCode = MAJOR*10000 + MINOR*100 +
 * PATCH (v0.1.9 -> 109), the same number as the iOS build.
 */
fun releaseVersion(): Pair<String, Int> {
    val raw = System.getenv("RELEASE_VERSION")?.takeIf { it.isNotBlank() }
        ?: runCatching {
            providers.exec { commandLine("git", "describe", "--tags", "--match", "v*", "--abbrev=7") }
                .standardOutput.asText.get().trim()
        }.getOrNull()?.takeIf { it.isNotEmpty() }
        ?: "v0.0.0-dev"
    val name = raw.removePrefix("v")
    val parts = name.substringBefore('-').split('.').map { it.toIntOrNull() ?: 0 } + listOf(0, 0, 0)
    return name to maxOf(1, parts[0] * 10000 + parts[1] * 100 + parts[2])
}

val (unVersionName, unVersionCode) = releaseVersion()

android {
    namespace = "app.onym.un"
    compileSdk = 36
    defaultConfig {
        applicationId = "app.onym.un"
        minSdk = 26
        targetSdk = 36
        versionCode = unVersionCode
        versionName = unVersionName
        ndk { abiFilters += listOf("arm64-v8a") }
    }
    buildTypes {
        // Unsigned: CI signs the bundle with the upload key (jarsigner), Play re-signs it.
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
    buildFeatures { compose = true; buildConfig = true }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    packaging { jniLibs { useLegacyPackaging = false } }
}

kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) } }

dependencies {
    implementation(platform("androidx.compose:compose-bom:2025.08.00"))
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.camera:camera-camera2:1.4.2")
    implementation("androidx.camera:camera-lifecycle:1.4.2")
    implementation("androidx.camera:camera-view:1.4.2")
    implementation("net.java.dev.jna:jna:5.17.0@aar")   // UniFFI Kotlin bindings load the library through JNA
}
