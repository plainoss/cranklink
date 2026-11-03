package app.plainoss.cranklink.shared

// Human-readable component types from Shimano model-name prefixes, e.g. "DU-EP800-CRG" -> "Motor".
// Keep in sync with web-instrument/src/domain/componentType.ts.
private val subtypes = listOf(
    "EW-WU" to "Wireless unit",
    "EW-SW" to "Shifter",
    "SM-EWW" to "Wireless unit",
    "SM-EW" to "Junction",
    "SM-BCR" to "Charger",
    "SM-BMR" to "Battery mount",
    "SM-PCE" to "PC interface",
)

private val types = mapOf(
    "DU" to "Motor",
    "RD" to "Rear derailleur",
    "FD" to "Front derailleur",
    "SW" to "Shifter",
    "ST" to "Shifter",
    "SC" to "Display",
    "BT" to "Battery",
    "BM" to "Battery mount",
    "EW" to "Junction",
    "MU" to "Hub motor unit",
    "FH" to "Freehub",
    "FC" to "Power meter",
)

/** The component type for a catalogued model name, or null when the prefix is unknown. */
fun componentType(model: String?): String? {
    if (model.isNullOrEmpty()) return null
    val name = model.uppercase()
    subtypes.firstOrNull { name.startsWith(it.first) }?.let { return it.second }
    return types[name.substringBefore('-')]
}

/** "Motor · DU-EP800-CRG", or just the model when its type is unknown. */
fun componentLabel(model: String): String = componentType(model)?.let { "$it · $model" } ?: model
