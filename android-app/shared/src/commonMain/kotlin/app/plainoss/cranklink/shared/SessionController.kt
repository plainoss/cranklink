package app.plainoss.cranklink.shared

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * A single exclusive BLE tuning session. Live values become writable only after
 * a complete, identity-bound response; uncertain operations require reconnecting.
 */
class SessionController(private val backend: AppBackend) {
    private val mutableState = MutableStateFlow(AppState())
    val state: StateFlow<AppState> = mutableState.asStateFlow()
    private val operation = Mutex()
    /** A region change staged only because the assist speed needed it, and the region staged before it. */
    private var autoRegion: AutoRegion? = null

    private data class AutoRegion(val before: String?, val staged: String?)

    suspend fun connect(master: Boolean = true) = operation.withLock {
        if (state.value.phase != AppPhase.Disconnected) return@withLock
        mutableState.value = state.value.copy(phase = AppPhase.Connecting, error = null, restartBike = false,
            status = "Connecting to your bike and reading its settings…")
        try {
            val result = execute(buildJsonObject { put("op", "connect"); put("master", master) })
            applySnapshot(result, expected = null, status = "Connected. Review changes before saving them to the bike.")
        } catch (error: Throwable) {
            failUncertain(error, "Connection failed. If the bike does not return to riding mode, turn it off and on again before reconnecting.")
        }
    }

    suspend fun refresh() = operation.withLock {
        val current = idleIdentity() ?: return@withLock
        if (state.value.staged.isNotEmpty()) {
            localError("Save or discard pending changes before reading the bike again.")
            return@withLock
        }
        mutableState.value = state.value.copy(phase = AppPhase.Reading, error = null,
            status = "Reading settings from the bike…")
        try {
            applySnapshot(execute("read", current), current, "Settings read from the bike.")
        } catch (error: Throwable) {
            failUncertain(error, "Read failed. Reconnect before changing settings.")
        }
    }

    private fun lockedOverBluetooth(label: String) =
        "$label can't be changed over this Bluetooth connection on this bike. Change it with a wired SM-PCE02 adapter in the cranklink web app or command line."

    private fun requiresMasterRouting(modelSlug: String, settingName: String): Boolean {
        val isE8000 = modelSlug in setOf("e8000", "e8000-pre41")
        return settingName in setOf("lamp", "lighting-time") ||
            (!isE8000 && settingName in setOf("destination", "tire-circumference"))
    }

    fun stage(name: String, value: String) {
        val current = state.value
        if (current.phase != AppPhase.Ready || current.identity == null) {
            localError(if (current.identity == null) "Connect to your bike first." else "Wait for the current operation to finish.")
            return
        }
        try {
            val plan = plan(current, current.identity, name, value)
            autoRegion = plan.auto
            val changes = plan.changes
            val summary = if (changes.isEmpty()) "No pending changes." else "${changeCount(changes.size)} ready for review."
            mutableState.value = current.copy(staged = changes, error = null,
                status = if (plan.note == null) summary else "${plan.note} $summary")
        } catch (error: IllegalArgumentException) {
            localError(error.message ?: "Invalid setting value.")
        } catch (error: IllegalStateException) {
            localError(error.message ?: "Invalid setting value.")
        }
    }

    /**
     * What staging [value] would do, without staging it: a note on any linked change
     * (such as the market region a faster speed needs), or the reason it would be refused.
     * Lets an editor explain the effect before the rider commits to it.
     */
    fun preview(name: String, value: String): Result<String?> {
        val current = state.value
        val identity = current.identity ?: return Result.failure(IllegalStateException("Connect to your bike first."))
        return runCatching { plan(current, identity, name, value).note }
    }

    private class StagePlan(val changes: Map<String, String>, val auto: AutoRegion?, val note: String?)

    private fun plan(current: AppState, identity: MotorIdentity, name: String, value: String): StagePlan {
        require(TuningRules.allowedName(name)) { "This setting is not available in the STEPS app." }
        val setting = current.settings.singleOrNull { it.name == name }
            ?: error("This setting is not supported by the connected motor.")
        require(identity.masterRouting || !requiresMasterRouting(identity.modelSlug, name)) {
            lockedOverBluetooth(setting.label)
        }
        require(name !in current.unavailable && name in current.values) { "Read this setting from the bike before changing it." }
        // Restoring the bike's own value removes the change without validating it:
        // undo must work even when that value is outside the writable range.
        val normalized = if (sameValue(setting, current.values[name], value)) value else TuningRules.validate(setting, value)
        val changes = current.staged.toMutableMap()
        fun put(key: String, next: String) {
            val spec = current.settings.single { it.name == key }
            if (sameValue(spec, current.values[key], next)) changes.remove(key) else changes[key] = next
        }
        val model = identity.modelSlug
        val region = current.settings.singleOrNull { it.name == "destination" }
        val linked = region != null && current.settings.any { it.name == "speed-limit" } &&
            "destination" in current.values && "speed-limit" in current.values
        // Forget an automatic region change the user has since replaced.
        var auto = autoRegion?.takeIf { changes["destination"] == it.staged }
        var note: String? = null
        if (linked && name == "speed-limit") {
            // Recompute from the user's own region choice, so undoing the speed
            // also undoes a region change that only the faster speed needed.
            auto?.let { if (it.before == null) changes.remove("destination") else changes["destination"] = it.before }
            auto = null
            val selected = (changes["destination"] ?: current.values["destination"])?.toDoubleOrNull()?.toInt()
            val speedText = formatSettingValue("speed-limit", normalized, model)
            // Only a region this connection can write may be changed on the rider's behalf;
            // otherwise Save would reject the region the speed silently pulled in.
            val speed = normalized.toDouble()
            val compatible = if (region!!.writable) TuningRules.destinationForSpeed(model, speed, selected, region.choices)
                else selected?.takeIf { TuningRules.speedRange(model, it)?.contains(speed) == true }
            if (compatible == null) {
                val range = TuningRules.speedRange(model, selected)
                throw IllegalArgumentException(if (!region.writable && range != null)
                    "${region.label} can't be changed over this connection, so assist speed must stay within " +
                        "${formatNumber(range.start)}–${formatNumber(range.endInclusive)} km/h."
                    else TuningRules.maxRegionSpeed(model, region.choices)?.let {
                        "No market region on this motor allows $speedText. The highest is ${formatNumber(it)} km/h."
                    } ?: "No market region on this motor allows $speedText.")
            }
            if (compatible != selected) {
                val before = changes["destination"]
                put("destination", compatible.toString())
                auto = AutoRegion(before, changes["destination"])
                note = "Market region changes to ${formatSettingValue("destination", compatible.toString(), model)} to allow $speedText."
            }
        } else if (linked && name == "destination") {
            auto = null
            val range = TuningRules.speedRange(model, normalized.toDouble().toInt())
            val speed = (changes["speed-limit"] ?: current.values["speed-limit"])?.toDoubleOrNull()
            if (range != null && speed != null && speed !in range) {
                val fitted = TuningRules.validate(current.settings.single { it.name == "speed-limit" }, formatNumber(speed.coerceIn(range)))
                put("speed-limit", fitted)
                note = "Assist speed changes to ${formatSettingValue("speed-limit", fitted, model)} to fit this market region."
            }
        } else if (model in setOf("e8000", "e7000")) {
            if (name == "assist-pattern") {
                val p = normalized.toIntOrNull()
                if (p == 2) {
                    if (current.settings.any { it.name == "boost-level" }) put("boost-level", "2")
                    if (current.settings.any { it.name == "trail-level" }) put("trail-level", "0")
                } else if (p == 3) {
                    if (current.settings.any { it.name == "boost-level" }) put("boost-level", "1")
                    if (current.settings.any { it.name == "trail-level" }) put("trail-level", "1")
                }
            } else if (name == "boost-level" || name == "trail-level") {
                val currentPattern = (changes["assist-pattern"] ?: current.values["assist-pattern"])?.toIntOrNull()
                if (currentPattern != 4 && current.settings.any { it.name == "assist-pattern" }) {
                    put("assist-pattern", "4")
                }
            }
        }
        put(name, normalized)
        TuningRules.validateCombination(identity, current.values, changes)
        return StagePlan(changes, auto, note)
    }

    fun discard() {
        if (state.value.phase != AppPhase.Ready) return
        autoRegion = null
        mutableState.value = state.value.copy(staged = emptyMap(), error = null, status = "Pending changes discarded.")
    }

    fun dismissError() {
        mutableState.value = state.value.copy(error = null)
    }

    fun acknowledgeRestart() {
        mutableState.value = state.value.copy(restartBike = false)
    }

    /** Reload local backup files after an import; no bike connection is required. */
    suspend fun refreshBackups() = operation.withLock {
        try {
            val backups = parseBackups(execute("backups")) ?: error("Missing backup list.")
            mutableState.value = state.value.copy(backups = backups)
        } catch (error: Throwable) {
            if (error is CancellationException) throw error
            localError(error.message ?: "Could not read the local backup list.")
        }
    }

    suspend fun commit() = operation.withLock {
        val identity = idleIdentity() ?: return@withLock
        val before = state.value
        if (before.staged.isEmpty()) return@withLock
        try {
            before.staged.forEach { (name, value) ->
                val setting = before.settings.singleOrNull { it.name == name } ?: error("A pending setting is not supported by this motor.")
                require(name in before.values && name !in before.unavailable) { "A pending setting could not be read from the bike." }
                TuningRules.validate(setting, value)
            }
            TuningRules.validateCombination(identity, before.values, before.staged)
        } catch (error: IllegalArgumentException) {
            localError(error.message ?: "Review the pending values.")
            return@withLock
        } catch (error: IllegalStateException) {
            localError(error.message ?: "Review the pending values.")
            return@withLock
        }
        // Region must precede speed so the native validator sees the selected market.
        val ordered = before.staged.entries.sortedBy { if (it.key == "destination") 0 else 1 }
        val changes = buildJsonObject { ordered.forEach { put(it.key, it.value) } }
        mutableState.value = before.copy(phase = AppPhase.Saving, error = null,
            status = "Backing up, saving and checking ${changeCount(ordered.size)}…")
        try {
            val result = execute(buildJsonObject {
                put("op", "commit")
                put("master", identity.masterRouting)
                put("serial", identity.serial)
                put("changes", changes)
            })
            val parsed = parseSnapshot(result, identity)
            ordered.forEach { (name, value) ->
                val setting = parsed.settings.singleOrNull { it.name == name }
                    ?: error("Saved setting is missing from the response.")
                require(name !in parsed.unavailable && sameValue(setting, parsed.values[name], value)) {
                    "Saved values did not return complete verified readbacks."
                }
            }
            autoRegion = null
            mutableState.value = parsed.copy(status = "${changeCount(ordered.size)} saved and verified. Previous values are backed up.")
        } catch (error: Throwable) {
            // Lead with what the stopped batch left on the bike, as the web app does.
            val raw = error.message
            val outcome = raw?.let { message ->
                // Label operations in the native plan's order (android-app/native/src/lib.rs).
                val planned = ordered.map { it.key }.sortedWith(compareBy({ if (it == "destination") 0 else 1 }, { it }))
                saveFailureOutcome(message, planned.map { name -> before.settings.singleOrNull { it.name == name }?.label ?: name })
            }
            val reported = if (outcome == null || error is CancellationException) error
                else IllegalStateException("$outcome ${userFacingError(raw)}", error)
            failUncertain(reported, "Save stopped. Earlier changes may remain applied. Reconnect and read the bike before retrying.")
        }
    }

    suspend fun restore(file: String) = operation.withLock {
        val identity = idleIdentity() ?: return@withLock
        if (state.value.staged.isNotEmpty()) {
            localError("Save or discard pending changes before restoring a backup.")
            return@withLock
        }
        if (file !in state.value.backups || !validBackupName(file)) {
            localError("Select an existing backup from this app.")
            return@withLock
        }
        mutableState.value = state.value.copy(phase = AppPhase.Restoring, error = null,
            status = "Backing up current values and restoring the selected setting…")
        try {
            val result = execute(buildJsonObject {
                put("op", "restore")
                put("master", identity.masterRouting)
                put("serial", identity.serial)
                put("file", file)
            })
            applySnapshot(result, identity, "Backup restored and verified. The previous value was backed up first.")
        } catch (error: Throwable) {
            failUncertain(error, "Restore stopped. Reconnect and read the bike before retrying.")
        }
    }

    suspend fun disconnect() = withContext(NonCancellable) {
        operation.withLock {
            // A failed operation already released the native session; keep its error visible.
            if (state.value.phase == AppPhase.Disconnected) return@withLock
            // A STEPS bike can stay on its Shimano logo after master routing
            // ends; a power cycle returns it to riding mode.
            val restartBike = state.value.identity?.masterRouting == true
            mutableState.value = state.value.copy(phase = AppPhase.Disconnecting, error = null,
                status = "Disconnecting from the motor and Bluetooth…")
            try {
                val result = execute("disconnect")
                mutableState.value = AppState(backups = parseBackups(result) ?: state.value.backups,
                    status = "Disconnected.", restartBike = restartBike)
            } catch (error: Throwable) {
                val backups = state.value.backups
                mutableState.value = AppState(backups = backups, error = error.message ?: "Disconnect failed.",
                    status = "Could not confirm the motor disconnected. Check the bike before reconnecting.",
                    restartBike = restartBike)
                if (error is CancellationException) throw error
            }
        }
    }

    private fun idleIdentity(): MotorIdentity? {
        val current = state.value
        if (current.phase != AppPhase.Ready || current.identity == null) {
            localError(if (current.identity == null) "Connect to your bike first." else "Wait for the current operation to finish.")
            return null
        }
        return current.identity
    }

    private fun localError(message: String) {
        mutableState.value = state.value.copy(error = message)
    }

    private suspend fun failUncertain(error: Throwable, status: String) {
        val oldBackups = state.value.backups
        mutableState.value = AppState(backups = oldBackups, error = error.message ?: "The operation failed.", status = status,
            restartBike = state.value.identity?.masterRouting == true)
        // Cancellation invalidates the live state, but must not skip releasing
        // the native master session. Keep the original error after cleanup.
        withContext(NonCancellable) {
            // An interrupted batch may have produced new backups before failing.
            val backups = try { parseBackups(execute("backups")) } catch (_: Exception) { null }
            if (backups != null) mutableState.value = state.value.copy(backups = backups)
            try { execute("disconnect") } catch (_: Exception) { /* The original error explains recovery. */ }
        }
        if (error is CancellationException) throw error
    }

    private suspend fun execute(op: String, identity: MotorIdentity? = null): JsonObject = execute(buildJsonObject {
        put("op", op)
        if (identity != null) put("serial", identity.serial)
    })

    private suspend fun execute(request: JsonObject): JsonObject {
        val response = backend.execute(request)
        if (response["error"] is JsonPrimitive) error(response.string("error"))
        return response["result"] as? JsonObject ?: response
    }

    private fun applySnapshot(result: JsonObject, expected: MotorIdentity?, status: String) {
        autoRegion = null
        mutableState.value = parseSnapshot(result, expected).copy(status = status)
    }

    private fun parseSnapshot(result: JsonObject, expected: MotorIdentity?): AppState {
        val identity = TuningRules.parseIdentity(result["identity"] as? JsonObject ?: error("Missing motor identity."))
        if (expected != null) require(identity.serial == expected.serial && identity.modelSlug == expected.modelSlug
            && identity.modelSeries == expected.modelSeries) { "The response belongs to a different drive unit." }
        val settings = TuningRules.parseSchema(result["schema"] as? JsonArray ?: error("Missing motor settings schema."),
            identity.modelSlug, result["constraints"] as? JsonObject).map { setting ->
            if (!identity.masterRouting && requiresMasterRouting(identity.modelSlug, setting.name)) {
                setting.copy(writable = false)
            } else {
                setting
            }
        }
        require(settings.isNotEmpty()) { "The connected drive unit has no supported tuning settings." }
        val snapshot = result["snapshot"] as? JsonObject ?: error("Missing motor settings snapshot.")
        require(snapshot.int("slot") == identity.slot) { "The snapshot belongs to another motor slot." }
        val entries = snapshot["values"] as? JsonObject ?: error("Missing motor settings values.")
        val schemaByName = settings.associateBy { it.name }
        val values = entries.mapValues { (name, entry) ->
            val setting = schemaByName[name] ?: error("Unknown setting in motor snapshot: $name")
            val raw = (entry as? JsonObject)?.get("value") ?: error("Invalid motor setting value.")
            val value = valueText(raw)
            if (setting.valueType == "string" || setting.valueType == "date" || value == "unset") value
            else value.toDoubleOrNull()?.let { require(it.isFinite()); formatNumber(it) } ?: error("Invalid numeric setting value.")
        }
        val unavailable = (snapshot["unavailable"] as? JsonObject).orEmpty().mapValues { (name, reason) ->
            require(name in schemaByName) { "Unknown unavailable setting in motor snapshot." }
            valueText(reason)
        }
        require(values.keys.intersect(unavailable.keys).isEmpty()) { "A setting cannot be both available and unavailable." }
        require(settings.all { it.name in values || it.name in unavailable }) { "The settings snapshot is incomplete." }
        return AppState(phase = AppPhase.Ready, identity = identity, settings = settings,
            values = values, unavailable = unavailable, backups = parseBackups(result) ?: state.value.backups)
    }

    private fun parseBackups(result: JsonObject): List<String>? = (result["backups"] as? JsonArray)?.map {
        val file = (it as? JsonPrimitive)?.content ?: error("Invalid backup filename.")
        require(validBackupName(file)) { "Invalid backup filename." }
        file
    }?.distinct()

    private fun validBackupName(file: String): Boolean = file.isNotBlank() && file.length <= 255 &&
        file != "." && file != ".." && '/' !in file && '\\' !in file && file.all { it.code >= 32 }

    private fun sameValue(setting: TuningSetting, left: String?, right: String): Boolean =
        if (setting.valueType == "string" || setting.valueType == "date" || right == "unset") left == right
        else left?.toDoubleOrNull() == right.toDoubleOrNull()

    private fun changeCount(count: Int): String = "$count ${if (count == 1) "change" else "changes"}"
}

/**
 * The highest assist speed this session can stage: the fastest region it may switch to,
 * or the current region's maximum when the region can't be changed on this connection.
 */
fun AppState.reachableSpeedMax(): Double? {
    val model = identity?.modelSlug ?: return null
    val region = settings.singleOrNull { it.name == "destination" } ?: return null
    if (region.writable) return TuningRules.maxRegionSpeed(model, region.choices)
    val selected = (staged["destination"] ?: values["destination"])?.toDoubleOrNull()?.toInt() ?: return null
    return TuningRules.speedRange(model, selected)?.endInclusive
}
