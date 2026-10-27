import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

/** Google's published test ad units: the only ones a debug build may carry. */
val testBannerUnit = "ca-app-pub-3940256099942544/9214589741"
val testInterstitialUnit = "ca-app-pub-3940256099942544/1033173712"

/**
 * The production unit ids come from `-PARENA_ADMOB_*` or the environment of the release job. A
 * release built without them carries an all-zero id (valid in form, never filled, never Google's
 * test id), so the app still builds and no test ad can ship; the release checklist (Story 4.16)
 * runs `check-ads.mjs ids --require-production`.
 */
fun releaseId(name: String, unset: String): String =
    (providers.gradleProperty(name).orNull ?: providers.environmentVariable(name).orNull)
        ?.takeIf { it.isNotBlank() } ?: unset

android {
    compileSdk = 36
    namespace = "in.arena360.ads"
    defaultConfig {
        minSdk = 28
    }
    buildTypes {
        getByName("debug") {
            buildConfigField("String", "BANNER_UNIT_ID", "\"$testBannerUnit\"")
            buildConfigField("String", "INTERSTITIAL_UNIT_ID", "\"$testInterstitialUnit\"")
        }
        getByName("release") {
            val banner = releaseId("ARENA_ADMOB_BANNER_UNIT", "ca-app-pub-0000000000000000/0000000000")
            val interstitial = releaseId("ARENA_ADMOB_INTERSTITIAL_UNIT", "ca-app-pub-0000000000000000/0000000000")
            buildConfigField("String", "BANNER_UNIT_ID", "\"$banner\"")
            buildConfigField("String", "INTERSTITIAL_UNIT_ID", "\"$interstitial\"")
            consumerProguardFiles("consumer-rules.pro")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

dependencies {
    // Pinned: the README pin table carries this version (`pnpm run check:pins`).
    implementation("com.google.android.gms:play-services-ads:25.5.0")
    implementation("androidx.core:core-ktx:1.16.0")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")
}
