# The Rust core (native/src/java_gatt.rs) calls AndroidGatt methods by name over JNI,
# and lib.rs exports Java_app_plainoss_cranklink_NativeCore_invoke. R8 cannot see
# either use, so keep those names unchanged.
-keep class app.plainoss.cranklink.AndroidGatt {
    public <methods>;
}
-keepclasseswithmembernames,includedescriptorclasses class app.plainoss.cranklink.NativeCore {
    native <methods>;
}
-keep class app.plainoss.cranklink.NativeCore { *; }
