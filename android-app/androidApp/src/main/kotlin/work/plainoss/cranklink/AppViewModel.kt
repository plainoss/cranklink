package work.plainoss.cranklink

import android.app.Application
import android.net.Uri
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.io.File
import java.io.InputStream
import java.util.concurrent.Executors
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.*
import work.plainoss.cranklink.shared.AppBackend
import work.plainoss.cranklink.shared.AppPhase
import work.plainoss.cranklink.shared.SessionController

/** JNI sessions are thread-local: every native call stays on this one worker. */
object NativeCore {
    private var loaded = false
    fun ensureLoaded() {
        if (!loaded) {
            System.loadLibrary("cranklink_android")
            loaded = true
        }
    }
    external fun invoke(gatt: AndroidGatt, request: String, backupDir: String): String
}

data class BackupInfo(val setting: String, val before: String, val serial: String, val model: String)

class AppViewModel(application: Application) : AndroidViewModel(application) {
    private val nativeDispatcher = Executors.newSingleThreadExecutor().asCoroutineDispatcher()
    private val backupDir = File(application.filesDir, "backups").apply { mkdirs() }
    @Volatile private var gatt: AndroidGatt? = null
    private var demoBackend = DemoBackend()
    private val _demo = MutableStateFlow(false)
    val demo = _demo.asStateFlow()
    private val _nearby = MutableStateFlow<List<NearbyBike>>(emptyList())
    val nearby = _nearby.asStateFlow()
    private val _scanning = MutableStateFlow(false)
    val scanning = _scanning.asStateFlow()
    private val _notice = MutableStateFlow<String?>(null)
    val notice = _notice.asStateFlow()
    private val _backupFiles = MutableStateFlow<List<String>>(emptyList())
    val backupFiles = _backupFiles.asStateFlow()
    private val _backupInfo = MutableStateFlow<Map<String, BackupInfo>>(emptyMap())
    val backupInfo = _backupInfo.asStateFlow()
    private val backupUpdates = Mutex()
    private data class CachedBackup(val modified: Long, val size: Long, val info: BackupInfo)
    private val backupCache = mutableMapOf<String, CachedBackup>()
    private var scanner: BleScanner? = null
    private var scanJob: Job? = null
    private var sessionJob: Job? = null
    private var disconnectQueued = false

    private val backend = object : AppBackend {
        override suspend fun execute(request: JsonObject): JsonObject = withContext(nativeDispatcher) {
            val operation = request["op"]?.jsonPrimitive?.content
            // Backups remain available while disconnected, including APKs without native libraries.
            if (operation == "backups") {
                updateBackups()
                return@withContext buildJsonObject {
                    putJsonArray("backups") { if (!_demo.value) _backupFiles.value.forEach { add(it) } }
                }
            }
            if (_demo.value) return@withContext demoBackend.execute(request)
            val transport = gatt
            if (transport == null && operation == "disconnect") {
                return@withContext buildJsonObject {
                    put("disconnected", true)
                    putJsonArray("backups") { _backupFiles.value.forEach { add(it) } }
                }
            }
            try {
                NativeCore.ensureLoaded()
                checkNotNull(transport) { "Choose a bike before connecting." }
                if (operation == "connect") transport.open()
                val reply = Json.parseToJsonElement(
                    NativeCore.invoke(transport, request.toString(), backupDir.absolutePath),
                ).jsonObject
                reply["error"]?.let { error(it.jsonPrimitive.content) }
                reply["result"]?.jsonObject ?: error("The motor returned an empty result.")
            } catch (error: Throwable) {
                // Release any retained master session before closing a healthy
                // radio. Native failures may already have performed this cleanup.
                if (error !is LinkageError && transport != null) {
                    runCatching { NativeCore.invoke(transport, "{\"op\":\"disconnect\"}", backupDir.absolutePath) }
                }
                transport?.close()
                if (error is LinkageError) throw IllegalStateException(
                    "This app build is missing Bluetooth support. Install a complete APK for this device.", error,
                )
                throw error
            } finally {
                if (operation == "disconnect") {
                    transport?.close()
                    if (gatt === transport) gatt = null
                }
                updateBackups()
            }
        }
    }
    val controller = SessionController(backend)

    init { viewModelScope.launch { updateBackups() } }

    fun scan() {
        stopScan()
        _notice.value = null
        _nearby.value = emptyList()
        scanner = BleScanner(getApplication(), { _nearby.value = it }, {
            _notice.value = it
            _scanning.value = false
        })
        _scanning.value = true
        try {
            scanner?.start()
            scanJob = viewModelScope.launch {
                delay(10_100)
                _scanning.value = false
            }
        } catch (error: Exception) {
            _scanning.value = false
            _notice.value = error.message ?: "Bluetooth scan failed."
        }
    }

    fun stopScan() {
        scanJob?.cancel()
        scanJob = null
        scanner?.stop()
        scanner = null
        _scanning.value = false
    }

    fun connect(bike: NearbyBike, pin: String) {
        if (!canConnect()) return
        stopScan()
        _demo.value = false
        lateinit var transport: AndroidGatt
        transport = AndroidGatt(getApplication(), bike.address, pin) { message ->
            transportLost(transport, message)
        }
        gatt = transport
        run { controller.connect() }
    }

    fun startDemo() {
        if (!canConnect()) return
        stopScan()
        gatt?.close()
        gatt = null
        _demo.value = true
        demoBackend = DemoBackend()
        run { controller.connect() }
    }

    fun refresh() = run { controller.refresh() }
    fun commit() = run { controller.commit() }
    fun restore(file: String) = run { controller.restore(file) }
    fun disconnect() = run { releaseSession() }

    /** Finish the bounded read/connect, then release master routing before closing GATT. */
    fun cancelConnection() {
        if (disconnectQueued || controller.state.value.phase !in setOf(AppPhase.Connecting, AppPhase.Reading)) return
        disconnectQueued = true
        val message = "Finishing the current operation before disconnecting…"
        _notice.value = message
        sessionJob = viewModelScope.launch {
            try {
                releaseSession()
            } finally {
                disconnectQueued = false
                if (_notice.value == message) _notice.value = null
            }
        }
    }
    fun showNotice(message: String?) { _notice.value = message }

    private fun run(action: suspend () -> Unit) {
        if (sessionJob?.isActive == true) return
        sessionJob = viewModelScope.launch { action() }
    }

    private fun canConnect(): Boolean {
        if (sessionJob?.isActive == true || controller.state.value.phase != AppPhase.Disconnected) {
            _notice.value = "Disconnect the current session before choosing another bike."
            return false
        }
        return true
    }

    private suspend fun releaseSession() {
        val transport = gatt
        try {
            controller.disconnect()
        } finally {
            transport?.close()
            if (gatt === transport) gatt = null
            _demo.value = false
        }
    }

    private fun transportLost(transport: AndroidGatt, message: String) {
        viewModelScope.launch {
            // A busy operation handles its own uncertain result. Waiting also covers a loss
            // racing the last successful readback before that operation publishes Ready.
            sessionJob?.join()
            if (gatt !== transport || _demo.value || controller.state.value.phase != AppPhase.Ready) return@launch
            releaseSession()
            controller.dismissError() // A missing remote link can make protocol cleanup unconfirmable.
            _notice.value = "Bluetooth link lost. Reconnect to read your bike again. $message"
        }
    }

    private suspend fun updateBackups() = withContext(Dispatchers.IO) {
        backupUpdates.withLock {
            val files = backupDir.listFiles().orEmpty()
                .filter { it.isFile && validBackupName(it.name) }
                .sortedByDescending { it.lastModified() }
            val names = files.map { it.name }
            backupCache.keys.retainAll(names.toSet())
            val metadata = buildMap {
                files.forEach { file ->
                    val modified = file.lastModified()
                    val size = file.length()
                    val cached = backupCache[file.name]?.takeIf { it.modified == modified && it.size == size }
                    val info = cached?.info ?: readBackupInfo(file, size)
                    if (info != null) {
                        backupCache[file.name] = CachedBackup(modified, size, info)
                        put(file.name, info)
                    } else backupCache.remove(file.name)
                }
            }
            // An unreadable file remains visible/exportable; only restore metadata is omitted.
            _backupFiles.value = names
            _backupInfo.value = metadata
        }
    }

    private fun readBackupInfo(file: File, size: Long): BackupInfo? {
        if (size !in 1..64 * 1024L) return null
        return runCatching {
            val bytes = file.inputStream().use { readBoundedBackup(it, 64 * 1024) }
            val json = Json.parseToJsonElement(bytes.decodeToString(throwOnInvalidSequence = true)).jsonObject
            fun text(key: String): String {
                val value = json[key] as? JsonPrimitive ?: error("Missing backup metadata")
                require(value.isString && value.content.isNotBlank() && value.content.length <= 256)
                return value.content
            }
            require(text("format") in setOf("cranklink-setting-backup-v1", "esteps-setting-backup-v1"))
            val before = json["before"] as? JsonPrimitive ?: error("Missing original value")
            BackupInfo(
                setting = text("setting"),
                before = if (before is JsonNull) "unset" else before.content.also { require(it.length <= 256) },
                serial = text("serial"),
                model = text("settings_model"),
            )
        }.getOrNull()
    }

    fun importBackup(uri: Uri) = viewModelScope.launch {
        try {
            withContext(Dispatchers.IO) {
                val bytes = getApplication<Application>().contentResolver.openInputStream(uri)?.use {
                    readBoundedBackup(it)
                } ?: error("Cannot open the selected file.")
                Json.parseToJsonElement(bytes.decodeToString(throwOnInvalidSequence = true)).jsonObject
                val temporary = File.createTempFile("import-", ".tmp", backupDir)
                try {
                    java.io.FileOutputStream(temporary).use { stream ->
                        stream.write(bytes)
                        stream.fd.sync()
                    }
                    val file = File(backupDir, "${temporary.name.removeSuffix(".tmp")}.backup.json")
                    check(temporary.renameTo(file)) { "Could not store the imported backup." }
                } finally {
                    temporary.delete()
                }
            }
            updateBackups()
            controller.refreshBackups()
            _notice.value = "Backup imported. Its model and serial will be checked before restore."
        } catch (error: Exception) {
            if (error is CancellationException) throw error
            _notice.value = error.message ?: "Could not import backup."
        }
    }

    fun exportBackup(file: String, uri: Uri) = viewModelScope.launch {
        try {
            withContext(Dispatchers.IO) {
                require(file in _backupFiles.value) { "Unknown backup." }
                getApplication<Application>().contentResolver.openOutputStream(uri)?.use { output ->
                    File(backupDir, file).inputStream().use { it.copyTo(output) }
                } ?: error("Cannot write to the selected destination.")
            }
            _notice.value = "Backup exported."
        } catch (error: Exception) {
            if (error is CancellationException) throw error
            _notice.value = error.message ?: "Could not export backup."
        }
    }

    override fun onCleared() {
        stopScan()
        // Let any bounded native operation finish before closing a healthy
        // radio: master routing must be restored through that connection.
        // Outlive viewModelScope cancellation and wait for the controller's
        // operation lock before releasing master routing on its JNI worker.
        CoroutineScope(SupervisorJob() + nativeDispatcher).launch {
            try {
                releaseSession()
            } finally {
                nativeDispatcher.close()
            }
        }
    }

    private fun validBackupName(name: String): Boolean = name.length <= 240 &&
        name.endsWith(".backup.json") && !name.startsWith('.') &&
        name.all { it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it in ".-_" }

    /** InputStream.readNBytes is unavailable on Android 8; read at most one byte past the limit. */
    private fun readBoundedBackup(input: InputStream, limit: Int = 1024 * 1024): ByteArray {
        val bytes = ByteArray(limit + 1)
        var size = 0
        while (size < bytes.size) {
            val count = input.read(bytes, size, bytes.size - size)
            if (count < 0) break
            if (count == 0) {
                val byte = input.read()
                if (byte < 0) break
                bytes[size++] = byte.toByte()
            } else size += count
        }
        require(size <= limit) { if (limit == 1024 * 1024) "Backup exceeds the 1 MiB limit." else "Backup metadata exceeds 64 KiB." }
        return bytes.copyOf(size)
    }
}

/** A separate in-memory bike: demo operations can never reach JNI or Bluetooth. */
private class DemoBackend : AppBackend {
    private val values = linkedMapOf(
        "destination" to "3", "speed-limit" to "25", "tire-circumference" to "2288",
        "lamp" to "1", "mounting-angle" to "0", "lighting-time" to "30",
        "light-output" to "1350", "display-speed-adjustment" to "15",
        "assist-pattern" to "5", "maintenance-alert" to "1", "maintenance-distance" to "1000",
    ).apply {
        for (profile in 1..2) for (mode in listOf("eco", "trail", "boost")) {
            put("profile$profile.$mode.torque", when (mode) { "eco" -> "40"; "trail" -> "70"; else -> "85" })
            put("profile$profile.$mode.character", when (mode) { "eco" -> "3"; "trail" -> "6"; else -> "9" })
            put("profile$profile.$mode.start", "3")
        }
    }

    override suspend fun execute(request: JsonObject): JsonObject {
        when (request["op"]?.jsonPrimitive?.content) {
            "disconnect" -> return buildJsonObject { put("disconnected", true) }
            "restore" -> error("Connect a real bike to restore a saved backup.")
            "commit" -> request["changes"]?.jsonObject?.forEach { (key, value) -> values[key] = value.jsonPrimitive.content }
        }
        return buildJsonObject {
            putJsonObject("identity") {
                put("model", "DU-EP800")
                put("model_name", "DU-EP800")
                put("settings_model", "ep800-crg")
                put("model_series", 0x23)
                put("model_unit", 0)
                put("serial", "DE0000000001")
                put("slot", 13)
                put("dcas_version", "4.2.0.0")
                put("rx_version", "4.1.11.0")
            }
            putJsonArray("schema") {
                values.forEach { (name, _) ->
                    val range = when {
                        name == "speed-limit" -> 1500..4500
                        name == "destination" -> 0..4
                        name == "tire-circumference" -> 1300..3000
                        name == "light-output" -> 135..2700
                        name == "display-speed-adjustment" -> -50..50
                        name == "maintenance-distance" -> 0..10000
                        name == "lighting-time" -> 0..180
                        name == "mounting-angle" -> 0..60
                        name.endsWith("torque") -> 20..85
                        name.endsWith("character") -> 1..10
                        name.endsWith("start") -> 1..5
                        name == "assist-pattern" -> 5..6
                        else -> 0..1
                    }
                    add(buildJsonObject {
                        put("name", name)
                        put("min_raw", range.first)
                        put("max_raw", range.last)
                        put("step_raw", if (name == "display-speed-adjustment") 5 else 1)
                        put("wire_units_per_input", if (name == "speed-limit") 100 else 1)
                        put("value_type", "number")
                        put("oem", name == "light-output")
                        put("read_only", false)
                        put("nullable", false)
                        put("unit", when {
                            name.endsWith("torque") -> "Nm"
                            name == "speed-limit" -> "km/h"
                            name == "tire-circumference" -> "mm"
                            name == "lighting-time" -> "min"
                            name == "maintenance-distance" -> "km"
                            name == "light-output" -> "0.01 W"
                            name == "display-speed-adjustment" -> "0.1 percent"
                            else -> ""
                        })
                        putJsonArray("choices") {
                            if (name == "destination") listOf(0, 1, 3, 4).forEach { add(it) }
                            if (name == "lamp" || name == "maintenance-alert") listOf(0, 1).forEach { add(it) }
                            if (name == "assist-pattern") listOf(5, 6).forEach { add(it) }
                        }
                    })
                }
            }
            putJsonObject("snapshot") {
                put("slot", 13)
                putJsonObject("values") { values.forEach { (key, value) -> putJsonObject(key) { put("value", value.toDouble()) } } }
                putJsonObject("unavailable") {}
            }
            putJsonArray("backups") {}
        }
    }
}
