# Consumer keep rules for bitkit-core Android bindings.
# Packaged into the AAR and applied automatically when a consuming app enables R8.

# JNA reads @Structure.FieldOrder and public fields by name, and constructs
# Structure, Structure$ByValue, and Structure$ByReference types reflectively.
-keepclassmembers class com.synonym.bitkitcore.** extends com.sun.jna.Structure {
    <fields>;
    <init>(...);
}

# JNA looks up Callback methods by name when building native function pointers.
-keepclassmembers class com.synonym.bitkitcore.** implements com.sun.jna.Callback {
    <methods>;
}

# Native.register maps remaining native methods by exact C symbol name.
-keepclasseswithmembers,allowshrinking,includedescriptorclasses class com.synonym.bitkitcore.UniffiLib {
    native <methods>;
}
-keepclasseswithmembers,allowshrinking,includedescriptorclasses class com.synonym.bitkitcore.IntegrityCheckingUniffiLib {
    native <methods>;
}

# @Structure.FieldOrder is read at runtime.
-keepattributes RuntimeVisibleAnnotations

# JNA's AAR references desktop AWT types that are absent on Android.
-dontwarn java.awt.Component
-dontwarn java.awt.GraphicsEnvironment
-dontwarn java.awt.HeadlessException
-dontwarn java.awt.Window
