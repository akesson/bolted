package dev.bolted.profileapp

import com.example.gen_profile_ffi.AvailabilityStash
import com.example.gen_profile_ffi.PlainDate
import com.example.gen_profile_ffi.AvailabilityRaw
import com.example.gen_profile_ffi.ProfileStashFfi
import com.example.gen_profile_ffi.ProfileValues
import com.example.gen_profile_ffi.TextFieldStashFfi
import org.json.JSONObject

/**
 * `ProfileStashFfi` ⇄ a JSON string, so the draft can live in a `SavedStateHandle` and survive
 * process death.
 *
 * **This file is a measurement, not a design.** BoltFFI emits `ProfileStashFfi` as a plain Kotlin
 * `data class` with no `Parcelable`, no `Serializable` and no `kotlinx.serialization` annotation, so
 * a shell that wants to persist one has to hand-write this. Every generated DTO a shell persists
 * costs a codec; `bolted-ffi` (step 10) should emit `@Parcelize` on Android and `Codable` on Apple,
 * and then this file deletes itself. Its length is the argument.
 *
 * Decoding is **total**: any malformed input yields `null` and the caller checks out a fresh draft.
 * The stash is the first untrusted input in the system — it is bytes the OS held while we were dead,
 * and on Android it can also be bytes an *older version of this app* wrote.
 */
object StashCodec {

    fun encode(stash: ProfileStashFfi): String =
        JSONObject().apply {
            // D27: the schema version rides the DTO now (`schemaVersion`), stamped by the core at
            // stash() time. The codec no longer owns a `FORMAT_VERSION` of its own — it just carries
            // the field through, and `store.acceptStash` is the gate. This is the version authority
            // moving into the generated DTO, before this hand-written codec is deleted (step 12 M4).
            put("schema_version", stash.schemaVersion.toLong())
            put("username", text(stash.username))
            put("name", text(stash.name))
            put("email", text(stash.email))
            put("availability", range(stash.availability))
            put("baseVersion", stash.baseVersion.toLong())
            put("orphaned", stash.orphaned)
        }.toString()

    fun decode(json: String): ProfileStashFfi? =
        runCatching {
            val o = JSONObject(json)
            // Structural (shape) failures still collapse to null here — a missing key throws and
            // `runCatching` swallows it, so the shell checks out fresh. The *version* gate is no
            // longer here: `schemaVersion` is carried through untouched and `acceptStash` decides.
            ProfileStashFfi(
                schemaVersion = o.getLong("schema_version").toUInt(),
                username = text(o.getJSONObject("username")),
                name = text(o.getJSONObject("name")),
                email = text(o.getJSONObject("email")),
                availability = range(o.getJSONObject("availability")),
                baseVersion = o.getLong("baseVersion").toULong(),
                orphaned = o.getBoolean("orphaned"),
            )
        }.getOrNull()

    /**
     * The simulated server's canonical values, also persisted — otherwise a restored VM would rebase
     * the stash onto the *seed* rather than onto what the "server" last said, and the process-death
     * tests would be testing nothing.
     */
    fun encodeValues(v: ProfileValues): String =
        JSONObject().apply {
            put("username", v.username)
            put("name", v.name)
            put("email", v.email)
            put("availability", dateRange(v.availability))
        }.toString()

    fun decodeValues(json: String): ProfileValues? =
        runCatching {
            val o = JSONObject(json)
            ProfileValues(
                username = o.getString("username"),
                name = o.getString("name"),
                email = o.getString("email"),
                availability = dateRange(o.getJSONObject("availability")),
            )
        }.getOrNull()

    // ---- per-raw-type codecs. Three of the four fields share one, because the stash names only
    // ---- `V::Raw`. The snapshot DTOs could not do that; see dto.rs.

    private fun text(s: TextFieldStashFfi): JSONObject =
        JSONObject().apply {
            putOpt("raw", s.raw)
            putOpt("base", s.base)
        }

    private fun text(o: JSONObject): TextFieldStashFfi =
        TextFieldStashFfi(raw = o.optNullableString("raw"), base = o.optNullableString("base"))

    private fun range(s: AvailabilityStash): JSONObject =
        JSONObject().apply {
            putOpt("raw", s.raw?.let(::dateRange))
            putOpt("base", s.base?.let(::dateRange))
        }

    private fun range(o: JSONObject): AvailabilityStash =
        AvailabilityStash(
            raw = o.optJSONObject("raw")?.let(::dateRange),
            base = o.optJSONObject("base")?.let(::dateRange),
        )

    private fun dateRange(r: AvailabilityRaw): JSONObject =
        JSONObject().apply {
            put("start", date(r.start))
            put("end", date(r.end))
        }

    private fun dateRange(o: JSONObject): AvailabilityRaw =
        AvailabilityRaw(start = date(o.getJSONObject("start")), end = date(o.getJSONObject("end")))

    private fun date(d: PlainDate): JSONObject =
        JSONObject().apply {
            put("y", d.year.toInt())
            put("m", d.month.toInt())
            put("d", d.day.toInt())
        }

    private fun date(o: JSONObject): PlainDate =
        PlainDate(
            year = o.getInt("y").toUShort(),
            month = o.getInt("m").toUByte(),
            day = o.getInt("d").toUByte(),
        )

    /** `org.json` stores an absent key and a JSON `null` differently; a stash needs `null` back. */
    private fun JSONObject.optNullableString(key: String): String? =
        if (isNull(key)) null else optString(key)
}
