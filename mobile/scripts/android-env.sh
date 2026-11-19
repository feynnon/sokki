# Source this: Java, Android SDK/NDK and Rust paths for building Sokki mobile.
export JAVA_HOME="$(brew --prefix openjdk@17)/libexec/openjdk.jdk/Contents/Home"
export ANDROID_HOME=/opt/homebrew/share/android-commandlinetools
export ANDROID_SDK_ROOT="$ANDROID_HOME"
export NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
# CMake (speech engine) finds the NDK through these.
export ANDROID_NDK_ROOT="$NDK_HOME" ANDROID_NDK="$NDK_HOME" CMAKE_POLICY_VERSION_MINIMUM=3.5
