import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

// Release signing (Google Play upload key): a properties file outside the
// repo with keyAlias, password and storeFile. Its path comes from
// SOKKI_ANDROID_KEYSTORE, else ~/.config/sokki/android-upload.properties.
// Without it, release builds come out unsigned.
val uploadKeyFile = file(
    System.getenv("SOKKI_ANDROID_KEYSTORE")
        ?: "${System.getProperty("user.home")}/.config/sokki/android-upload.properties"
)
val uploadKey = Properties().apply {
    if (uploadKeyFile.exists()) {
        uploadKeyFile.inputStream().use { load(it) }
    }
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

android {
    compileSdk = 37
    namespace = "dev.verbatim.mobile"
    defaultConfig {
        // Only encrypted connections: the phone link and the model download
        // don't use plain HTTP. Debug builds allow it for Tauri's dev server.
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        // The ID Android and Google Play know the app by; it can never change
        // after the first upload. The code's namespace (and Tauri's
        // identifier) stay dev.verbatim.mobile: renaming them would move
        // every Kotlin file for no visible gain.
        applicationId = "org.sokki.app"
        minSdk = 24
        targetSdk = 37
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    signingConfigs {
        if (uploadKeyFile.exists()) {
            create("upload") {
                keyAlias = uploadKey.getProperty("keyAlias")
                keyPassword = uploadKey.getProperty("password")
                storePassword = uploadKey.getProperty("password")
                storeFile = file(uploadKey.getProperty("storeFile"))
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            if (uploadKeyFile.exists()) {
                signingConfig = signingConfigs.getByName("upload")
            }
            optimization {
               enable = true
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
        // The keyboard uses newer Java APIs on older Android versions.
        isCoreLibraryDesugaringEnabled = true
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_17
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    // The Sokki keyboard (HeliBoard): mobile/keyboard
    implementation(project(":keyboard"))
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = file("tauri.build.gradle.kts"))
