plugins {
    id("com.android.application")
}

layout.buildDirectory =
    file(providers.gradleProperty("quickshareBuildRoot").get() + "/app")

android {
    namespace = "dev.omarchy.quickshare.probe"
    buildToolsVersion = "36.1.0"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.omarchy.quickshare.probe"
        minSdk = 31
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    lint {
        abortOnError = true
        checkAllWarnings = true
        checkDependencies = true
        warningsAsErrors = true
    }
}

dependencies {
    implementation("androidx.annotation:annotation:1.10.0")
    implementation("androidx.test:runner:1.7.0")
    implementation("androidx.test.uiautomator:uiautomator:2.4.0")
    implementation("com.google.android.gms:play-services-nearby:19.5.0")
    implementation("com.google.android.mobly:mobly-snippet-lib:1.4.0")
}
