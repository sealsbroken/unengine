# The Rust library is reached through JNA: UniFFI's generated bindings (uniffi.*) and JNA itself
# are found by reflection and by name from native code, so R8 keeps them whole.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uniffi.** { *; }
-keep interface app.onym.un.MobileLibrary { *; }
-dontwarn java.awt.**
