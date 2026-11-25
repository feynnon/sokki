// The Sokki keyboard on its own (the public repo feynnon/sokki-keyboard).
// Assembled from the Sokki monorepo by standalone/assemble.sh; develop there.
pluginManagement {
    repositories { google(); mavenCentral(); gradlePluginPortal() }
}
dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
        maven { url = uri("https://jitpack.io") } // HeliBoard's color picker
    }
}
rootProject.name = "sokki-keyboard"
include(":app", ":keyboard")
