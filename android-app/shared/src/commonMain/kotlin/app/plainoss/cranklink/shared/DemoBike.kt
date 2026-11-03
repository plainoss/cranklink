package app.plainoss.cranklink.shared

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

/** A value saved in the demo before it was overwritten, as a real save backs it up first. */
data class DemoBackup(val file: String, val setting: String, val before: String, val savedAt: Long)

/**
 * An in-memory DU-EP800 connected the way a real one is over Bluetooth: as bus
 * master, so market region, wheel size and every other setting cranklink supports
 * on this motor can be changed. Its schema is the core's own ([DEMO_EP800_SCHEMA]),
 * so ranges and steps match a real bike. It never reaches JNI or Bluetooth.
 */
class DemoBike(private val clock: () -> Long = { 0L }) : AppBackend {
    private val schema = Json.parseToJsonElement(DEMO_EP800_SCHEMA).jsonArray
    private val names = schema.map { it.jsonObject.string("name") }.toSet()
    private val values = initialValues().also { values ->
        require(values.keys == names) { "Demo values must cover exactly the demo schema." }
    }
    private val saved = mutableListOf<DemoBackup>()

    /** Backups made by demo saves and restores, newest first. */
    val backups: List<DemoBackup> get() = saved.toList()

    override suspend fun execute(request: JsonObject): JsonObject {
        when (request["op"]?.jsonPrimitive?.content) {
            "disconnect" -> return buildJsonObject { put("disconnected", true) }
            "backups" -> return buildJsonObject { putBackups() }
            "commit" -> {
                val changes = request["changes"]?.jsonObject ?: error("Missing changes.")
                changes.keys.forEach { require(it in names) { "Setting $it is not part of the demo bike." } }
                checkLinkedLimits(values + changes.mapValues { it.value.jsonPrimitive.content })
                changes.forEach { (name, value) -> write(name, value.jsonPrimitive.content) }
            }
            "restore" -> {
                val file = request["file"]?.jsonPrimitive?.content
                val backup = saved.firstOrNull { it.file == file } ?: error("Select an existing demo backup.")
                // Like the core, refuse a restore that would leave speed or power above the region's limits.
                checkLinkedLimits(values + (backup.setting to backup.before))
                write(backup.setting, backup.before)
            }
        }
        return snapshot()
    }

    private fun checkLinkedLimits(effective: Map<String, String>) {
        val destination = effective["destination"]?.toDoubleOrNull()?.toInt()
        val range = TuningRules.speedRange(MODEL, destination)
            ?: error("The demo bike has no assist-speed range for market region $destination.")
        val speed = effective["speed-limit"]?.toDoubleOrNull()
        require(speed != null && speed in range) {
            "${formatSettingValue("destination", destination.toString(), MODEL)} allows " +
                "${formatNumber(range.start)}–${formatNumber(range.endInclusive)} km/h, but assist speed would be " +
                "${formatSettingValue("speed-limit", effective["speed-limit"] ?: "unset", MODEL)}. Change the assist speed first."
        }
        if (destination == 3) require(listOf("eco", "trail", "boost").all { (effective["max-power.$it"]?.toDoubleOrNull() ?: 0.0) <= 400 }) {
            "Region 3 limits maximum motor power to 400 W. Lower it first."
        }
    }

    private fun write(name: String, value: String) {
        val before = values.getValue(name)
        if (before == value) return
        val number = saved.size + 1
        saved.add(0, DemoBackup("demo-$number-$name.backup.json", name, before, clock()))
        values[name] = value
    }

    private fun snapshot(): JsonObject = buildJsonObject {
        putJsonObject("identity") {
            put("model", "DU-EP800")
            put("model_name", "DU-EP800")
            put("settings_model", "ep800-crg")
            put("model_series", 0x23)
            put("model_unit", 0)
            put("serial", SERIAL)
            put("slot", 13)
            put("dcas_version", "4.2.0.0")
            put("rx_version", "4.2.0.0")
            // A real EP800 over Bluetooth connects as bus master, which is what
            // allows region and wheel-size changes without a wired adapter.
            put("master_routing", true)
        }
        put("schema", schema)
        putJsonObject("snapshot") {
            put("slot", 13)
            putJsonObject("values") {
                values.forEach { (name, value) ->
                    putJsonObject(name) { put("value", jsonValue(name, value)) }
                }
            }
            putJsonObject("unavailable") {}
        }
        putBackups()
    }

    private fun kotlinx.serialization.json.JsonObjectBuilder.putBackups() {
        putJsonArray("backups") { saved.forEach { add(JsonPrimitive(it.file)) } }
    }

    private fun jsonValue(name: String, value: String) = when {
        value == "unset" -> JsonNull
        name.endsWith(".name") || name == "maintenance-date" -> JsonPrimitive(value)
        else -> JsonPrimitive(value.toDouble())
    }

    private fun initialValues(): MutableMap<String, String> {
        val values = linkedMapOf(
            "lamp" to "1", "light-output" to "1350", "lighting-time" to "30", "walk-assist" to "2",
            "tire-circumference" to "2288", "mounting-angle" to "0",
            // Europe at 25 km/h, as most EP800 bikes ship.
            "destination" to "0", "assist-pattern" to "5", "speed-limit" to "25",
            "display-speed-adjustment" to "15",
            "maintenance-alert" to "1", "maintenance-distance" to "1000",
            "maintenance-year" to "27", "maintenance-month" to "4", "maintenance-day" to "1",
            "maintenance-date" to "2027-04-01",
            // Verified factory bounds, so OEM Factory Bounds can be explored too.
            "torque-minimum" to "20", "torque-maximum" to "85",
            "profile1.name" to "Trail", "profile2.name" to "Commute",
            "max-power.eco" to "250", "max-power.trail" to "400", "max-power.boost" to "500",
        )
        for (profile in 1..2) {
            for (suffix in listOf("", "-oem")) {
                values["profile$profile.display$suffix"] = "1"
                for ((mode, torque, character) in listOf(Triple("eco", 40, 2), Triple("trail", 70, 5), Triple("boost", 85, 8))) {
                    values["profile$profile.$mode.torque$suffix"] = torque.toString()
                    values["profile$profile.$mode.character$suffix"] = character.toString()
                    values["profile$profile.$mode.start$suffix"] = "2"
                }
            }
        }
        return values
    }

    companion object {
        const val SERIAL = "DE0000000001"
        private const val MODEL = "ep800-crg"
    }
}
