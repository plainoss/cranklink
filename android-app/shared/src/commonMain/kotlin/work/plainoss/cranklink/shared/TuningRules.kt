package work.plainoss.cranklink.shared

import kotlin.math.abs
import kotlin.math.round
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonPrimitive

/** This allowlist deliberately excludes drivetrain, component and firmware operations. */
internal object TuningRules {
    private val motorModels = mapOf(
        0x08 to "e6000", 0x0a to "e6001", 0x0c to "e6001", 0x12 to "e8000", 0x15 to "e6002",
        0x20 to "e6100", 0x21 to "e7000",
        0x23 to "ep800-crg", 0x30 to "ep600", 0x35 to "ep801", 0x37 to "ep500", 0x38 to "e5100",
    )
    private val names = setOf(
        "lamp", "light-output", "lighting-time", "walk-assist", "tire-circumference",
        "mounting-angle", "destination", "assist-pattern", "speed-limit", "display-speed-adjustment",
        "maintenance-alert", "maintenance-distance", "maintenance-year", "maintenance-month",
        "maintenance-day", "maintenance-date", "torque-minimum", "torque-maximum",
    )
    private val profile = Regex("profile[12]\\.(?:name|display(?:-oem)?|(?:eco|trail|boost)\\.(?:torque|character|start)(?:-oem)?)")
    private val power = Regex("max-power\\.(?:eco|trail|boost)")

    fun allowedName(name: String): Boolean = name in names || profile.matches(name) || power.matches(name)

    fun parseIdentity(raw: JsonObject): MotorIdentity {
        val series = raw.int("model_series")
        val family = motorModels[series] ?: error("This component is not a supported STEPS drive unit.")
        val slug = if (series == 0x15) {
            val components = raw.stringOrNull("dcas_version")?.takeIf {
                Regex("[0-9]+\\.[0-9]+\\.[0-9]+\\.[0-9]+").matches(it)
            }?.split('.')
            val version = components?.map { it.toIntOrNull() }
            require(version?.size == 4 && version.all { it != null && it in 0..65535 }) {
                "Cannot select E6002 settings without a valid drive-unit firmware version."
            }
            if (version[0]!! > 3 || (version[0] == 3 && version[1]!! >= 3)) "e6002" else "e6002-pre33"
        } else family
        require(raw.int("model_unit") == 0) { "Only STEPS drive units are supported." }
        require(raw.string("settings_model") == slug) { "The settings list does not match the connected motor." }
        val serial = raw.string("serial").uppercase()
        require(Regex("[0-9A-F]{12}").matches(serial)) { "Could not read a valid motor serial number." }
        val slot = raw.int("slot")
        require(slot in 0..31) { "The drive unit returned an invalid BLE slot." }
        val model = raw.stringOrNull("model_name") ?: slug.uppercase()
        return MotorIdentity(model, serial, slot, raw.stringOrNull("dcas_version") ?: "—",
            raw.stringOrNull("rx_version") ?: "—", slug, series,
            (raw["master_routing"] as? JsonPrimitive)?.booleanOrNull == true)
    }

    fun parseSchema(raw: JsonArray, model: String, constraints: JsonObject? = null): List<TuningSetting> {
        val settings = raw.map { element ->
            val spec = element as? JsonObject ?: error("Invalid settings schema.")
            val name = spec.string("name")
            require(allowedName(name)) { "Unsupported setting in motor schema: $name" }
            require(allowedForModel(name, model)) { "Setting $name is unavailable for this motor family." }
            val scale = spec.numberOrNull("wire_units_per_input") ?: 1.0
            require(scale.isFinite() && scale > 0) { "Invalid unit scale for $name." }
            val min = spec.numberOrNull("min_raw") ?: 0.0
            val max = spec.numberOrNull("max_raw") ?: min
            val step = spec.numberOrNull("step_raw") ?: 1.0
            require(min.isFinite() && max.isFinite() && step.isFinite() && max >= min && step > 0) {
                "Invalid bounds for $name."
            }
            val choices = (spec["choices"] as? JsonArray).orEmpty().map {
                val rawValue = (it as? JsonPrimitive)?.doubleOrNull ?: error("Invalid choice for $name.")
                formatNumber(rawValue / scale)
            }
            val presentation = settingPresentation(name, model)
            val modernTorque = isModern(model) && Regex("profile[12]\\.(?:eco|trail|boost)\\.torque(?:-oem)?").matches(name)
            val torqueChoices = if (modernTorque) modeTorqueLevels(name, verifiedTorqueLevels(
                constraints?.numberOrNull("torque_minimum"), constraints?.numberOrNull("torque_maximum"),
            )).map { it.toString() } else emptyList()
            val torqueUnavailable = modernTorque && torqueChoices.isEmpty()
            TuningSetting(name, presentation.label, section(name), min / scale, max / scale, step / scale,
                if (modernTorque) torqueChoices else choices, presentation.unit, !spec.bool("read_only") && !torqueUnavailable,
                spec.stringOrNull("value_type") ?: "number", spec.bool("nullable"), spec.bool("oem"),
                description = presentation.description + if (torqueUnavailable) " Torque editing is unavailable because verified factory torque levels could not be established." else "",
                displayScale = presentation.displayScale,
                displayOffset = presentation.displayOffset)
        }
        require(settings.map { it.name }.distinct().size == settings.size) { "Duplicate settings in motor schema." }
        return settings
    }

    private fun allowedForModel(name: String, model: String): Boolean = when (model) {
        "e6000", "e6001", "e6002-pre33", "e8000", "e6100", "e7000" -> name in setOf("lamp", "tire-circumference", "mounting-angle")
        "e6002" -> name in setOf("lamp", "tire-circumference", "mounting-angle", "destination")
        "e5100" -> name in setOf("lamp", "lighting-time", "walk-assist", "tire-circumference", "mounting-angle",
            "destination", "speed-limit", "display-speed-adjustment") || name.startsWith("maintenance-")
        "ep600", "ep801", "ep500" -> name !in setOf("light-output", "torque-minimum", "torque-maximum") &&
            !name.startsWith("max-power.")
        "ep800-crg" -> true
        else -> false
    }

    fun validate(setting: TuningSetting, value: String): String {
        require(setting.writable) { "${setting.label} is read-only." }
        if (value == "unset") {
            require(setting.nullable) { "${setting.label} needs a value and cannot be cleared." }
            return value
        }
        if (setting.valueType == "string") {
            require(value.length <= 10 && value.all { it.code in 32..126 }) {
                "Use up to 10 English letters, digits, spaces or punctuation for the profile name."
            }
            return value
        }
        if (setting.valueType == "date") {
            require(validDate(value)) { "Enter a valid date as YYYY-MM-DD, or type unset to clear the date." }
            return value
        }
        val number = value.toDoubleOrNull()
        require(number != null && number.isFinite()) { "Enter a valid number for ${setting.label}." }
        require(number >= setting.min && number <= setting.max) {
            "${setting.label} must be ${withDisplayUnit("${formatNumber(setting.displayMin)}–${formatNumber(setting.displayMax)}", setting.unit)}."
        }
        val steps = (number - setting.min) / setting.step
        require(abs(steps - round(steps)) < 0.000001) {
            "${setting.label} changes in steps of ${withDisplayUnit(formatNumber(setting.displayStep), setting.unit)}."
        }
        val normalized = formatNumber(number)
        require(setting.choices.isEmpty() || setting.choices.any { it.toDoubleOrNull() == number }) {
            "Choose a supported value for ${setting.label}."
        }
        return normalized
    }

    fun validateCombination(identity: MotorIdentity, values: Map<String, String>, changes: Map<String, String>) {
        require("maintenance-date" !in changes ||
            changes.keys.none { it in setOf("maintenance-year", "maintenance-month", "maintenance-day") }) {
            "Change the complete maintenance date or its individual fields in one save, not both."
        }
        val effective = values + changes
        if (identity.modelSlug !in e600xModels && ("speed-limit" in changes || "destination" in changes)) {
            val speed = effective["speed-limit"]?.toDoubleOrNull()
            val destination = effective["destination"]?.toIntOrNull()
            if (speed != null) {
                val range = speedRange(identity.modelSlug, destination)
                require(range != null && speed in range) { "Assist speed does not fit the selected market region." }
            }
        }
        val minTorque = effective["torque-minimum"]?.toDoubleOrNull()
        val maxTorque = effective["torque-maximum"]?.toDoubleOrNull()
        if (minTorque != null && maxTorque != null) {
            require(minTorque <= maxTorque) { "Minimum torque exceeds maximum torque." }
        }
        changes.filterKeys { ".torque" in it }.forEach { (name, text) ->
            val torque = text.toDoubleOrNull() ?: return@forEach
            if (minTorque != null) require(torque >= minTorque) { "Torque is below the factory minimum." }
            if (maxTorque != null) require(torque <= maxTorque) { "Torque is above the factory maximum." }
            // Dynamic modern levels and drivetrain constraints remain authoritative in Rust.
            if (isModern(identity.modelSlug) && minTorque != null && maxTorque != null) {
                val levels = verifiedTorqueLevels(minTorque, maxTorque)
                check(levels.isNotEmpty()) { "Torque levels for factory bounds ${formatNumber(minTorque)}–${formatNumber(maxTorque)} Nm are not verified. Torque changes are unavailable for this motor." }
                val permitted = modeTorqueLevels(name, levels)
                require(permitted.any { it.toDouble() == torque }) { "Choose a supported basic-mode torque level." }
            }
        }
    }

    fun speedRange(model: String, destination: Int?): ClosedFloatingPointRange<Double>? = if (model in e600xModels) null else when (destination) {
        0, 3 -> 15.0..25.0
        1 -> 19.0..32.0
        2 -> if (isModern(model)) 14.0..24.0 else null
        4 -> if (!isModern(model)) 15.0..25.0 else null
        5 -> if (isModern(model)) 19.0..45.0 else null
        else -> null
    }

    /** The first region permitting [speed], preferring [preferred], as the web app picks it. */
    fun destinationForSpeed(model: String, speed: Double, preferred: Int?, choices: List<String>): Int? =
        (listOfNotNull(preferred) + (0..5)).firstOrNull { destination ->
            (choices.isEmpty() || choices.any { it.toDoubleOrNull() == destination.toDouble() }) &&
                speedRange(model, destination)?.contains(speed) == true
        }

    internal fun isModern(model: String) = model in setOf("ep600", "ep801", "ep500")
    private val e600xModels = setOf("e6000", "e6001", "e6002-pre33", "e6002")
    private fun verifiedTorqueLevels(minimum: Double?, maximum: Double?): List<Int> = when {
        minimum == 20.0 && maximum == 85.0 -> listOf(20, 27, 34, 42, 49, 56, 63, 70, 78, 85)
        minimum == 20.0 && maximum == 50.0 -> listOf(20, 24, 27, 30, 33, 36, 40, 43, 46, 50)
        else -> emptyList()
    }
    private fun modeTorqueLevels(name: String, levels: List<Int>): List<Int> = when {
        ".eco." in name -> levels.take(8)
        ".boost." in name -> levels.drop(2)
        else -> levels
    }

    private fun validDate(value: String): Boolean {
        if (!Regex("20[0-9]{2}-[0-9]{2}-[0-9]{2}").matches(value)) return false
        val year = value.substring(0, 4).toInt()
        val month = value.substring(5, 7).toInt()
        val day = value.substring(8, 10).toInt()
        val days = when (month) {
            1, 3, 5, 7, 8, 10, 12 -> 31
            4, 6, 9, 11 -> 30
            2 -> if (year % 4 == 0) 29 else 28
            else -> 0
        }
        return day in 1..days
    }

    private fun section(name: String): String = when {
        name.startsWith("maintenance-") -> "maintenance"
        name.endsWith("-oem") || name.startsWith("torque-") || name.startsWith("max-power.") -> "oem"
        name.startsWith("profile") || name == "assist-pattern" -> "profiles"
        else -> "geometry"
    }

}

internal fun JsonObject.string(name: String): String = stringOrNull(name) ?: error("Missing $name.")
internal fun JsonObject.stringOrNull(name: String): String? = (this[name] as? JsonPrimitive)?.takeUnless { it is JsonNull }?.content
internal fun JsonObject.int(name: String): Int = (this[name] as? JsonPrimitive)?.intOrNull ?: error("Missing $name.")
internal fun JsonObject.numberOrNull(name: String): Double? = (this[name] as? JsonPrimitive)?.doubleOrNull
internal fun JsonObject.bool(name: String): Boolean = (this[name] as? JsonPrimitive)?.booleanOrNull ?: false
internal fun valueText(element: JsonElement): String = if (element == JsonNull) "unset" else element.jsonPrimitive.content
internal fun formatNumber(number: Double): String = if (number == number.toLong().toDouble()) number.toLong().toString() else number.toString()
