package dev.bolted.profileprobe

/**
 * Marks a probe that deliberately exercises **undefined behaviour** and may kill the instrumented
 * process. Excluded from the default suite (`notAnnotation`) so a native crash cannot take the other
 * probes' results down with it; run on its own via `mise run test:android:hazard`.
 */
@Retention(AnnotationRetention.RUNTIME)
@Target(AnnotationTarget.CLASS, AnnotationTarget.FUNCTION)
annotation class HazardProbe
