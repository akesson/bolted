package dev.bolted.profileapp

/**
 * Marks a suite that may only run on physical silicon.
 *
 * `mise run test:android:app` excludes these (`notAnnotation`), because the headless Gradle-Managed
 * Device is an emulator and step 05 already measured that. `mise run bench:android:device` includes
 * *only* these (`annotation`), and refuses an `emulator-*` serial before Gradle starts.
 *
 * Same shape as step 05's `@HazardProbe`, and for the same reason: a suite whose premise is "the
 * environment is different" must not be able to run in the wrong environment by accident.
 */
@Retention(AnnotationRetention.RUNTIME)
@Target(AnnotationTarget.CLASS, AnnotationTarget.FUNCTION)
annotation class PhysicalDevice
