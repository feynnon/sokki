// The Sokki keyboard: HeliBoard (GPL-3.0, see UPSTREAM) as a library inside
// the Sokki Android app, so one install has the app and the keyboard.
// Changed from HeliBoard's app/build.gradle.kts: library instead of
// application, build values the code reads (BuildConfig) set by hand, our
// NDK, no test setup.

import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
    // Tauri's project turns off AGP's built-in Kotlin (gradle.properties),
    // so the Kotlin plugin is applied here.
    id("org.jetbrains.kotlin.android")
    kotlin("plugin.serialization") version "2.4.0"
    kotlin("plugin.compose") version "2.4.0"
}

android {
    compileSdk = 37
    namespace = "helium314.keyboard.latin"

    defaultConfig {
        minSdk = 24
        ndk {
            abiFilters.clear()
            abiFilters.addAll(listOf("arm64-v8a", "x86_64"))
        }
        consumerProguardFiles("proguard-rules.pro")
        // HeliBoard reads these from BuildConfig; libraries don't get them.
        // Must match the app's applicationId (src-tauri/gen/android/app).
        buildConfigField("String", "APPLICATION_ID", "\"org.sokki.app\"")
        buildConfigField("int", "VERSION_CODE", "4201")
        buildConfigField("String", "VERSION_NAME", "\"4.2-sokki\"")
    }

    buildFeatures {
        viewBinding = true
        buildConfig = true
        compose = true
    }

    externalNativeBuild {
        ndkBuild {
            path = File("src/main/jni/Android.mk")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
        isCoreLibraryDesugaringEnabled = true
    }

    lint {
        abortOnError = false
        // Release builds run "lint vital" first. On this module it crashes
        // inside lint itself (NoSuchMethodError in JavaDocParser, AGP 9.3.1 +
        // Kotlin 2.4.0), not on a finding, and fails the build. The app's
        // release still asks for this module's lint vital, so the tasks are
        // switched off below too.
        checkReleaseBuilds = false
    }
}

// See lint { } above: lint vital crashes on this module, so skip it.
tasks.configureEach {
    if (name.startsWith("lintVital")) enabled = false
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_17
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.17.0")
    implementation("androidx.recyclerview:recyclerview:1.4.0")
    implementation("androidx.autofill:autofill:1.3.0")
    implementation("androidx.viewpager2:viewpager2:1.1.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")
    implementation(platform("androidx.compose:compose-bom:2025.11.01"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.navigation:navigation-compose:2.9.8")
    implementation("sh.calvin.reorderable:reorderable:3.1.0")
    implementation("com.github.skydoves:colorpicker-compose:1.1.3")
}
