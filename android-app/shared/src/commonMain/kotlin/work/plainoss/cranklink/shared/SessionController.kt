package work.plainoss.cranklink.shared

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

    suspend fun connect() = operation.withLock {
        if (state.value.phase != AppPhase.Disconnected) return@withLock
        mutableState.value = state.value.copy(phase = AppPhase.Connecting, error = null,
            status = "Connecting to your bike and reading its settings…")
        try {
            val result = execute(buildJsonObject { put("op", "connect"); put("master", true) })
            applySnapshot(result, expected = null, status = "Connected. Review changes before saving them to the bike.")
        } catch (error: Throwable) {
            failUncertain(error, "Connection failed. Reconnect to try again.")
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

    fun stage(name: String, value: String) {
        val current = state.value
        if (current.phase != AppPhase.Ready || current.identity == null) {
            localError(if (current.identity == null) "Connect to your bike first." else "Wait for the current operation to finish.")
            return
        }
        try {
            require(TuningRules.allowedName(name)) { "This setting is not available in the STEPS app." }
            val setting = current.settings.singleOrNull { it.name == name }
                ?: error("This setting is not supported by the connected motor.")
            require(name !in current.unavailable && name in current.values) { "Read this setting from the bike before changing it." }
            // Restoring the bike's own value removes the change without validating it:
            // undo must work even when that value is outside the writable range.
            val normalized = if (sameValue(setting, current.values[name], value)) value else TuningRules.validate(setting, value)
            val changes = current.staged.toMutableMap()
            if (sameValue(setting, current.values[name], normalized)) changes.remove(name) else changes[name] = normalized
            TuningRules.validateCombination(current.identity, current.values, changes)
            mutableState.value = current.copy(staged = changes, error = null,
                status = if (changes.isEmpty()) "No pending changes." else "${changeCount(changes.size)} ready for review.")
        } catch (error: IllegalArgumentException) {
            localError(error.message ?: "Invalid setting value.")
        } catch (error: IllegalStateException) {
            localError(error.message ?: "Invalid setting value.")
        }
    }

    fun discard() {
        if (state.value.phase != AppPhase.Ready) return
        mutableState.value = state.value.copy(staged = emptyMap(), error = null, status = "Pending changes discarded.")
    }

    fun dismissError() {
        mutableState.value = state.value.copy(error = null)
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
            mutableState.value = state.value.copy(phase = AppPhase.Disconnecting, error = null,
                status = "Disconnecting from the motor and Bluetooth…")
            try {
                val result = execute("disconnect")
                mutableState.value = AppState(backups = parseBackups(result) ?: state.value.backups,
                    status = "Disconnected.")
            } catch (error: Throwable) {
                val backups = state.value.backups
                mutableState.value = AppState(backups = backups, error = error.message ?: "Disconnect failed.",
                    status = "Could not confirm the motor disconnected. Check the bike before reconnecting.")
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
        mutableState.value = AppState(backups = oldBackups, error = error.message ?: "The operation failed.", status = status)
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
        mutableState.value = parseSnapshot(result, expected).copy(status = status)
    }

    private fun parseSnapshot(result: JsonObject, expected: MotorIdentity?): AppState {
        val identity = TuningRules.parseIdentity(result["identity"] as? JsonObject ?: error("Missing motor identity."))
        if (expected != null) require(identity.serial == expected.serial && identity.modelSlug == expected.modelSlug
            && identity.modelSeries == expected.modelSeries) { "The response belongs to a different drive unit." }
        val settings = TuningRules.parseSchema(result["schema"] as? JsonArray ?: error("Missing motor settings schema."),
            identity.modelSlug, result["constraints"] as? JsonObject)
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
