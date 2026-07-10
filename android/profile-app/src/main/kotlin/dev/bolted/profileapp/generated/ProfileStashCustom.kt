package dev.bolted.profileapp.generated

import com.example.gen_profile_ffi.AvailabilityRaw
import com.example.gen_profile_ffi.AvailabilityStash
import com.example.gen_profile_ffi.PlainDate
import org.json.JSONObject

/**
 * **Hand-written — NOT generated. Do not delete when regenerating.**
 *
 * The composite half of the stash codec: `Profile.availability` is a `DateRange` (raw `(Date, Date)`),
 * a composite value object whose shape `bolted-ffi-gen` does not know and does not guess (D20/D25).
 * This is the Kotlin analogue of `gen-profile-ffi`'s `custom.rs`, one language out: the generated
 * `ProfileStashCodec` references these four `encode*`/`decode*` functions by name, and a missing one
 * is a Kotlin compile error — never a field that silently crosses as whatever the generator felt like.
 *
 * The generator emits every text field's (de)serialisation itself; only the composite lives here.
 */
object ProfileStashCustom {

    fun encodeAvailabilityStash(s: AvailabilityStash): JSONObject =
        JSONObject().apply {
            putOpt("raw", s.raw?.let(::encodeAvailabilityRaw))
            putOpt("base", s.base?.let(::encodeAvailabilityRaw))
        }

    fun decodeAvailabilityStash(o: JSONObject): AvailabilityStash =
        AvailabilityStash(
            raw = o.optJSONObject("raw")?.let(::decodeAvailabilityRaw),
            base = o.optJSONObject("base")?.let(::decodeAvailabilityRaw),
        )

    fun encodeAvailabilityRaw(r: AvailabilityRaw): JSONObject =
        JSONObject().apply {
            put("start", encodeDate(r.start))
            put("end", encodeDate(r.end))
        }

    fun decodeAvailabilityRaw(o: JSONObject): AvailabilityRaw =
        AvailabilityRaw(
            start = decodeDate(o.getJSONObject("start")),
            end = decodeDate(o.getJSONObject("end")),
        )

    private fun encodeDate(d: PlainDate): JSONObject =
        JSONObject().apply {
            put("y", d.year.toInt())
            put("m", d.month.toInt())
            put("d", d.day.toInt())
        }

    private fun decodeDate(o: JSONObject): PlainDate =
        PlainDate(
            year = o.getInt("y").toUShort(),
            month = o.getInt("m").toUByte(),
            day = o.getInt("d").toUByte(),
        )
}
