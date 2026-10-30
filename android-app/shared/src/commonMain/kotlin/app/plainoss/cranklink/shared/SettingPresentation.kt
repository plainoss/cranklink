package app.plainoss.cranklink.shared

import kotlin.math.round

/** Human-facing metadata, also available for backups when no bike schema is loaded. */
data class SettingPresentation(
    val label: String,
    val description: String,
    val unit: String = "",
    val displayScale: Double = 1.0,
    val displayOffset: Double = 0.0,
)

fun settingPresentation(name: String, model: String? = null): SettingPresentation {
    if (name == "destination" && model == "e6002") return SettingPresentation("Market region",
        "Motor market selection. Choose the region that applies where the bike is ridden.")
    standardPresentations[name]?.let { return it }
    val profile = Regex("profile([12])\\.(eco|trail|boost)\\.(torque|character|start)(-oem)?").matchEntire(name)
    if (profile != null) {
        val (number, mode, field, factorySuffix) = profile.destructured
        val modeLabel = mode.replaceFirstChar { it.uppercase() }
        val factory = factorySuffix.isNotEmpty()
        val fieldLabel = when (field) {
            "torque" -> "peak torque"
            "character" -> "assist character"
            else -> "start response"
        }
        val explanation = when (field) {
            "torque" -> "Peak motor torque allowed in $modeLabel mode. Higher values raise the torque limit, within the motor's factory limits."
            "character" -> "How motor assistance responds to your pedaling effort in $modeLabel mode. This is separate from the peak torque limit."
            else -> "How quickly assistance builds when you begin pedaling in $modeLabel mode. This controls startup response, separate from peak torque."
        }
        return SettingPresentation(
            "Profile $number $modeLabel $fieldLabel${if (factory) " (factory)" else ""}",
            if (factory) "Factory default: $explanation" else explanation,
            if (field == "torque") "Nm" else "level",
        )
    }
    val display = Regex("profile([12])\\.display(-oem)?").matchEntire(name)
    if (display != null) {
        val (number, factorySuffix) = display.destructured
        return SettingPresentation("Profile $number mode names${if (factorySuffix.isNotEmpty()) " (factory)" else ""}",
            "Choose whether the cycle computer labels the modes Eco / Normal / High or Eco / Trail / Boost." +
                if (factorySuffix.isNotEmpty()) " This is the factory default." else "")
    }
    val profileName = Regex("profile([12])\\.name").matchEntire(name)
    if (profileName != null) {
        return SettingPresentation("Profile ${profileName.groupValues[1]} name",
            "Name shown for this assist profile on the cycle computer. Use up to 10 English letters, digits, spaces or punctuation.")
    }
    val power = Regex("max-power\\.(eco|trail|boost)").matchEntire(name)
    if (power != null) {
        val mode = power.groupValues[1].replaceFirstChar { it.uppercase() }
        return SettingPresentation("$mode maximum motor power",
            "Motor power limit for $mode mode. Higher values raise the allowed power.", "W")
    }
    val label = name.replace('.', ' ').replace('-', ' ').split(' ')
        .joinToString(" ") { it.replaceFirstChar { char -> char.uppercase() } }
    return SettingPresentation(label, "Value reported by the connected motor for $label.")
}

/** Physical number shown in controls; this never changes a stored or transmitted value. */
fun TuningSetting.displayNumber(backend: Double): Double = presentationNumber(backend, displayScale, displayOffset)

val TuningSetting.displayMin: Double get() = displayNumber(min)
val TuningSetting.displayMax: Double get() = displayNumber(max)
val TuningSetting.displayStep: Double get() = presentationNumber(step, displayScale, 0.0)

/** Convert an editable physical value back into the existing canonical backend contract. */
fun TuningSetting.backendInput(displayText: String): String {
    if (valueType == "string" || valueType == "date" || displayText == "unset") {
        return TuningRules.validate(this, displayText)
    }
    val display = displayText.trim().toDoubleOrNull()
    require(display != null && display.isFinite()) { "Enter a valid number for $label." }
    require(displayScale.isFinite() && displayScale > 0 && displayOffset.isFinite()) { "Invalid display units for $label." }
    val canonical = (display - displayOffset) / displayScale
    // Validate before snapping so extra precision is rejected, rather than rounded into a write.
    TuningRules.validate(this, formatNumber(canonical))
    val snapped = min + round((canonical - min) / step) * step
    return TuningRules.validate(this, formatNumber(snapped))
}

/** Whether cranklink could write this canonical value back, as a restore or an undo of a save would. */
fun TuningSetting.canWriteBack(canonical: String): Boolean =
    runCatching { TuningRules.validate(this, canonical) }.isSuccess

/** Initial edit text, preserving names, dates and unset values byte for byte. */
fun TuningSetting.inputText(backendText: String): String {
    if (valueType == "string" || valueType == "date" || backendText == "unset") return backendText
    val value = backendText.toDoubleOrNull()?.takeIf { it.isFinite() } ?: return backendText
    return formatNumber(displayNumber(value))
}

/** Format canonical snapshot/backup values without requiring an active connection. */
fun formatSettingValue(name: String, canonical: String, model: String? = null): String {
    if (canonical == "unset") return "Not set"
    if (name.endsWith(".name") || name == "maintenance-date") return canonical
    val number = canonical.toDoubleOrNull()?.takeIf { it.isFinite() } ?: return canonical
    if (number == number.toInt().toDouble()) {
        val choice = number.toInt()
        val label = when (name) {
            "lamp" -> mapOf(0 to "Off", 1 to "On", 2 to "Auto")[choice]
            "maintenance-alert" -> mapOf(0 to "Off", 1 to "On")[choice]
            "walk-assist" -> mapOf(0 to "Disabled", 2 to "Enabled")[choice]
            "assist-pattern" -> mapOf(5 to "Standard", 6 to "Sport")[choice]
            "destination" -> marketLabel(model, choice)
            else -> if (Regex("profile[12]\\.display(?:-oem)?").matches(name)) {
                mapOf(0 to "Eco / Normal / High", 1 to "Eco / Trail / Boost")[choice]
            } else null
        }
        if (label != null) return label
    }
    val presentation = settingPresentation(name, model)
    return withDisplayUnit(formatNumber(presentationNumber(number, presentation.displayScale, presentation.displayOffset)), presentation.unit)
}

/** E6002 markets have no verified assist-speed range; other motors use the core's range table. */
private fun marketLabel(model: String?, choice: Int): String? {
    if (model == "e6002") return mapOf(0 to "Europe (EU)", 1 to "United States (US)", 4 to "Korea")[choice]
    val name = marketName(model, choice) ?: "Region $choice"
    if (model == null) return name
    val range = TuningRules.speedRange(model, choice) ?: return null
    return "$name · ${formatNumber(range.start)}–${formatNumber(range.endInclusive)} km/h"
}

/** Only codes whose market is known; 3 and legacy 4 keep their number. */
internal fun marketName(model: String?, choice: Int): String? = when (choice) {
    0 -> "Europe"
    1 -> "United States"
    2 -> if (model != null && TuningRules.isModern(model)) "Japan" else null
    5 -> if (model != null && TuningRules.isModern(model)) "US Class 3" else null
    else -> null
}

internal fun withDisplayUnit(value: String, unit: String): String = when (unit) {
    "" -> value
    "%", "°" -> "$value$unit"
    "level" -> "Level $value"
    else -> "$value $unit"
}

private fun presentationNumber(value: Double, scale: Double, offset: Double): Double {
    val physical = value * scale + offset
    // Decimal wire units such as 0.01 W should never display binary floating-point tails.
    return round(physical * 1_000_000_000.0) / 1_000_000_000.0
}

private val standardPresentations = mapOf(
    "lamp" to SettingPresentation("Wired light output",
        "Controls power to lights connected to the motor. Auto is available on supported models."),
    "light-output" to SettingPresentation("Light power allocation",
        "Power allocated to the motor's wired light output. Higher values allow more power for connected lights.", "W", displayScale = 0.01),
    "lighting-time" to SettingPresentation("Lighting reserve",
        "Time reserved for the lights when battery charge is too low for motor assistance. Higher values set a longer reserve.", "min"),
    "walk-assist" to SettingPresentation("Walk assist",
        "Enables low-speed motor assistance while you push the bike."),
    "tire-circumference" to SettingPresentation("Wheel circumference",
        "Distance traveled per wheel revolution, used to calculate speed and distance. Enter the measured rollout of the wheel.", "mm"),
    "mounting-angle" to SettingPresentation("Motor mounting angle",
        "Installation angle of the motor in the frame. Match this value to the motor's physical mounting angle.", "°"),
    "destination" to SettingPresentation("Market region",
        "Selects the market region and its permitted assist-speed range. Choose the region where the bike is ridden."),
    "assist-pattern" to SettingPresentation("Assist pattern",
        "Selects the motor response pattern shared by both assist profiles."),
    "speed-limit" to SettingPresentation("Assist speed limit",
        "Speed at which motor assistance cuts out. Higher values allow assistance to a higher speed within the selected region's limits.", "km/h"),
    "display-speed-adjustment" to SettingPresentation("Speedometer calibration",
        "Percentage correction applied to the speed shown by the cycle computer.", "%", displayScale = 0.1),
    "maintenance-alert" to SettingPresentation("Service reminder",
        "Enables the service reminder shown by the cycle computer."),
    "maintenance-distance" to SettingPresentation("Service reminder distance",
        "Distance setting used by the service reminder.", "km"),
    "maintenance-year" to SettingPresentation("Service year",
        "Year of the scheduled service date.", displayOffset = 2000.0),
    "maintenance-month" to SettingPresentation("Service month",
        "Month of the scheduled service date, from 1 for January to 12 for December."),
    "maintenance-day" to SettingPresentation("Service day",
        "Day of the month for the scheduled service date."),
    "maintenance-date" to SettingPresentation("Service date",
        "Scheduled service date. Use YYYY-MM-DD, or unset to clear the date."),
    "torque-minimum" to SettingPresentation("Minimum profile torque",
        "Lower bound for assist-profile peak torque. Profile torque values must stay at or above this limit.", "Nm"),
    "torque-maximum" to SettingPresentation("Maximum profile torque",
        "Upper bound for assist-profile peak torque. Profile torque values must stay at or below this limit.", "Nm"),
)
