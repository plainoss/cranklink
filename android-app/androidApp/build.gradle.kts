plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// Release CI sets the version and the upload key through the environment; local builds keep 1 / 0.1.0.
val releaseCode = providers.environmentVariable("ANDROID_VERSION_CODE").orElse("1").get().toInt()
require(releaseCode in 1..2100000000) { "ANDROID_VERSION_CODE must be between 1 and 2100000000" }
val releaseName = providers.environmentVariable("ANDROID_VERSION_NAME").orElse("0.1.0").get()
require(releaseName.isNotBlank()) { "ANDROID_VERSION_NAME must not be blank" }
val signingVariables = listOf("ANDROID_KEYSTORE_PATH", "ANDROID_KEYSTORE_PASSWORD", "ANDROID_KEY_ALIAS", "ANDROID_KEY_PASSWORD")
val signingValues = signingVariables.associateWith { providers.environmentVariable(it).orNull }
val hasReleaseSigning = signingValues.values.all { !it.isNullOrBlank() }

android {
    namespace = "app.plainoss.cranklink"
    compileSdk = 36
    defaultConfig {
        applicationId = "app.plainoss.cranklink"
        minSdk = 26
        targetSdk = 36
        versionCode = releaseCode
        versionName = releaseName
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }
    signingConfigs {
        if (hasReleaseSigning) {
            create("upload") {
                storeFile = file(signingValues.getValue("ANDROID_KEYSTORE_PATH")!!)
                storePassword = signingValues.getValue("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = signingValues.getValue("ANDROID_KEY_ALIAS")
                keyPassword = signingValues.getValue("ANDROID_KEY_PASSWORD")
            }
        }
    }
    buildTypes {
        release {
            if (hasReleaseSigning) signingConfig = signingConfigs.getByName("upload")
        }
    }
    buildFeatures { compose = true }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    packaging { resources.excludes += "/META-INF/{AL2.0,LGPL2.1}" }
}

dependencies {
    implementation(project(":shared"))
    implementation(platform("androidx.compose:compose-bom:2025.04.01"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.9.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.8.1")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation(platform("androidx.compose:compose-bom:2025.04.01"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
}

val buildNative by tasks.registering(Exec::class) {
    group = "build"
    description = "Build the BLE settings core for arm64 and the Android emulator"
    workingDir(rootProject.projectDir)
    commandLine("bash", "scripts/build-native.sh", "--release")
    inputs.files(fileTree(rootProject.file("native/src")), fileTree(rootProject.file("../src")))
    inputs.files(rootProject.file("native/Cargo.toml"), rootProject.file("native/Cargo.lock"),
        rootProject.file("../Cargo.toml"), rootProject.file("../Cargo.lock"), rootProject.file("scripts/build-native.sh"))
    outputs.files("src/main/jniLibs/arm64-v8a/libcranklink_android.so", "src/main/jniLibs/x86_64/libcranklink_android.so")
}
tasks.named("preBuild").configure { dependsOn(buildNative) }
tasks.matching { it.name == "preReleaseBuild" }.configureEach {
    doFirst { check(hasReleaseSigning) { "Release signing requires: ${signingVariables.joinToString()}" } }
}
