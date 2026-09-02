plugins {
    id("com.android.application") version "9.0.1" apply false
}

layout.buildDirectory =
    file(providers.gradleProperty("quickshareBuildRoot").get() + "/root")

allprojects {
    dependencyLocking {
        lockAllConfigurations()
    }
}
