// A keyboard-only app: the Sokki keyboard (../keyboard) with its own app ID,
// so it installs next to the Sokki app. Voice typing needs the speech engine
// (../voice, Rust): built with cargo-ndk before the APK when cargo is there,
// else the keyboard builds without it and the mic says the engine is missing.
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    compileSdk = 37
    namespace = "org.sokki.keyboard"
    defaultConfig {
        applicationId = providers.gradleProperty("sokki.appId").get()
        minSdk = 24
        targetSdk = 36
        versionCode = 4201
        versionName = "4.2-sokki"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
        isCoreLibraryDesugaringEnabled = true
    }
    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "../keyboard/proguard-rules.pro")
        }
    }
}

kotlin { compilerOptions { jvmTarget.set(JvmTarget.JVM_17) } }

dependencies {
    implementation(project(":keyboard"))
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")
}

// The speech engine: libmobile_lib.so (the name SokkiVoice loads) into jniLibs.
val voice = tasks.register<Exec>("buildVoice") {
    workingDir = file("../voice")
    commandLine("cargo", "ndk", "-t", "arm64-v8a", "-t", "x86_64", "-o", file("src/main/jniLibs").path, "build", "--release")
    isIgnoreExitValue = false
    onlyIf { providers.gradleProperty("sokki.voice").getOrElse("true") == "true" }
}
tasks.named("preBuild") { dependsOn(voice) }
