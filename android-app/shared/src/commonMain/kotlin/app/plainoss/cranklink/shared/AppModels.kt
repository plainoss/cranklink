package app.plainoss.cranklink.shared

import kotlinx.serialization.json.JsonObject

/** The native implementation owns BLE framing, authentication and durable backups. */
interface AppBackend {
    suspend fun execute(request: JsonObject): JsonObject
}

enum class AppPhase {
    Disconnected, Connecting, Ready, Reading, Saving, Restoring, Disconnecting,
}

data class MotorIdentity(
    val model: String,
    val serial: String,
    val slot: Int,
    val dcasVersion: String,
    val rxVersion: String,
    val modelSlug: String,
    val modelSeries: Int,
    /** The gateway routed this session as bus master; the bike needs a restart afterwards. */
    val masterRouting: Boolean = false,
)

data class TuningSetting(
    val name: String,
    val label: String,
    val section: String,
    val min: Double,
    val max: Double,
    val step: Double,
    val choices: List<String>,
    val unit: String,
    val writable: Boolean,
    val valueType: String = "number",
    val nullable: Boolean = false,
    val oem: Boolean = false,
    val description: String = "",
    /** Presentation only: backend values, bounds and requests keep their canonical units. */
    val displayScale: Double = 1.0,
    val displayOffset: Double = 0.0,
)

data class AppState(
    val phase: AppPhase = AppPhase.Disconnected,
    val identity: MotorIdentity? = null,
    val settings: List<TuningSetting> = emptyList(),
    val values: Map<String, String> = emptyMap(),
    val unavailable: Map<String, String> = emptyMap(),
    val staged: Map<String, String> = emptyMap(),
    val backups: List<String> = emptyList(),
    val error: String? = null,
    val status: String = "Connect to your STEPS bike over Bluetooth.",
    /** A master-routing session ended; the rider must turn the bike off and on to ride. */
    val restartBike: Boolean = false,
)
