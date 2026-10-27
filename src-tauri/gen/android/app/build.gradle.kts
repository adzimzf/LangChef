import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// Release signing credentials come from keystore.properties at the repo root
// (gitignored) or from LANGCHEF_KEYSTORE_* environment variables. Never commit them.
val keystoreProperties = Properties().apply {
    val propFile = rootProject.file("../../../keystore.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

fun signingValue(key: String, env: String): String? =
    keystoreProperties.getProperty(key) ?: System.getenv(env)

val releaseStoreFile = signingValue("storeFile", "LANGCHEF_KEYSTORE_FILE")

android {
    signingConfigs {
        if (releaseStoreFile != null) {
            create("release") {
                storeFile = rootProject.file("../../../").resolve(releaseStoreFile)
                storePassword = signingValue("storePassword", "LANGCHEF_KEYSTORE_PASSWORD")
                keyAlias = signingValue("keyAlias", "LANGCHEF_KEY_ALIAS")
                keyPassword = signingValue("keyPassword", "LANGCHEF_KEY_PASSWORD")
            }
        }
    }
    compileSdk = 36
    namespace = "app.langchef"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "app.langchef"
        // CI preview builds set LANGCHEF_APPLICATION_ID_SUFFIX / LANGCHEF_APP_NAME so they
        // install next to the release app instead of replacing it.
        System.getenv("LANGCHEF_APPLICATION_ID_SUFFIX")?.takeIf { it.isNotBlank() }?.let {
            applicationIdSuffix = it
        }
        val appName = System.getenv("LANGCHEF_APP_NAME")?.takeIf { it.isNotBlank() } ?: "LangChef"
        resValue("string", "app_name", appName)
        resValue("string", "main_activity_title", appName)
        minSdk = 24
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            isMinifyEnabled = true
            signingConfig = signingConfigs.findByName("release")
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
    packaging {
        jniLibs {
            // Only the arm64 runtime is used (the app is built for arm64); the Java bindings are not used at all.
            excludes += listOf(
                "lib/armeabi-v7a/libonnxruntime*.so",
                "lib/x86/libonnxruntime*.so",
                "lib/x86_64/libonnxruntime*.so",
                "lib/*/libonnxruntime4j_jni.so",
            )
        }
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    // ONNX Runtime for the offline Japanese voice (tts.rs loads libonnxruntime.so at run time).
    // Keep the version in step with ORT_VERSION in tts.rs and the `ort` crate in Cargo.toml.
    implementation("com.microsoft.onnxruntime:onnxruntime-android:1.31.1")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    // Google Sign-In
    implementation("com.google.android.gms:play-services-auth:21.4.0")
    implementation("androidx.credentials:credentials:1.3.0")
    implementation("androidx.credentials:credentials-play-services-auth:1.3.0")
    implementation("com.google.android.libraries.identity.googleid:googleid:1.1.1")
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    implementation("com.google.code.gson:gson:2.13.2")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.7.3")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-play-services:1.7.3")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")