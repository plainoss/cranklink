package work.plainoss.cranklink

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material.icons.automirrored.filled.BluetoothSearching
import androidx.compose.material.icons.automirrored.filled.Undo
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.viewmodel.compose.viewModel
import work.plainoss.cranklink.shared.*

private val Pine = Color(0xFF153D31)
private val Lime = Color(0xFFC8F27D)
private val Paper = Color(0xFFF6F5F0)
private val Ink = Color(0xFF1E2925)
private val Muted = Color(0xFF69756D)

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            MaterialTheme(colorScheme = lightColorScheme(
                primary = Pine, onPrimary = Color.White, secondary = Pine,
                tertiary = Color(0xFF637B43), background = Paper, surface = Color.White,
                onBackground = Ink, onSurface = Ink, surfaceVariant = Color(0xFFEBEEE7),
            )) {
                val vm: AppViewModel = viewModel()
                val state by vm.controller.state.collectAsStateWithLifecycle()
                val writing = state.phase == AppPhase.Saving || state.phase == AppPhase.Restoring
                DisposableEffect(writing) {
                    if (writing) window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                    onDispose { window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
                }
                CranklinkApp(vm, state)
            }
        }
    }
}

@Composable
private fun CranklinkApp(vm: AppViewModel, state: AppState) {
    val context = LocalContext.current
    val demo by vm.demo.collectAsStateWithLifecycle()
    val nearby by vm.nearby.collectAsStateWithLifecycle()
    val scanning by vm.scanning.collectAsStateWithLifecycle()
    val notice by vm.notice.collectAsStateWithLifecycle()
    val backups by vm.backupFiles.collectAsStateWithLifecycle()
    val backupInfo by vm.backupInfo.collectAsStateWithLifecycle()
    var page by rememberSaveable { mutableIntStateOf(0) }
    var showScan by remember { mutableStateOf(false) }
    var selectedBike by remember { mutableStateOf<NearbyBike?>(null) }
    var review by remember { mutableStateOf(false) }
    var discard by remember { mutableStateOf(false) }
    var disconnect by remember { mutableStateOf(false) }
    var restoreFile by remember { mutableStateOf<String?>(null) }
    var exportFile by remember { mutableStateOf<String?>(null) }
    val busy = state.phase !in listOf(AppPhase.Ready, AppPhase.Disconnected)
    val connected = state.identity != null
    BackHandler(enabled = connected || busy || page != 0) {
        when (state.phase) {
            AppPhase.Saving, AppPhase.Restoring, AppPhase.Disconnecting ->
                vm.showNotice("Wait for the current operation to finish before disconnecting.")
            AppPhase.Connecting, AppPhase.Reading -> vm.cancelConnection()
            else -> if (page == 2) page = 0 else if (connected) disconnect = true else page = 0
        }
    }
    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner, vm) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_STOP) vm.stopScan()
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }
    val permissions = if (Build.VERSION.SDK_INT >= 31) arrayOf(
        Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_CONNECT,
    ) else arrayOf(Manifest.permission.ACCESS_FINE_LOCATION)
    val permissionLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { granted ->
        if (granted.values.all { it }) vm.scan()
        else vm.showNotice("Allow Nearby devices access in Android settings to find your bike.")
    }
    val importLauncher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let(vm::importBackup)
    }
    val exportLauncher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        val file = exportFile
        if (uri != null && file != null) vm.exportBackup(file, uri)
        exportFile = null
    }
    fun scan() {
        showScan = true
        if (permissions.all { ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED }) vm.scan()
        else permissionLauncher.launch(permissions)
    }

    Scaffold(
        containerColor = Paper,
        topBar = {
            Row(Modifier.fillMaxWidth().statusBarsPadding().padding(horizontal = 24.dp, vertical = 17.dp),
                verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(35.dp).clip(RoundedCornerShape(11.dp)).background(Pine), contentAlignment = Alignment.Center) {
                    Icon(Icons.Default.Bolt, null, tint = Lime, modifier = Modifier.size(24.dp))
                }
                Spacer(Modifier.width(10.dp))
                Text("cranklink", fontSize = 23.sp, fontWeight = FontWeight.Bold, letterSpacing = (-0.7).sp)
                Spacer(Modifier.weight(1f))
                if (connected) {
                    StatusPill(if (demo) "DEMO" else "BLE", if (demo) Color(0xFFF1E6C5) else Lime)
                    IconButton(onClick = { disconnect = true }, enabled = !busy) {
                        Icon(Icons.Default.PowerSettingsNew, "Disconnect bike", tint = Pine)
                    }
                } else Text("STEPS", color = Muted, fontSize = 12.sp)
            }
        },
        bottomBar = {
            if (connected || page == 2) Column {
                if (state.staged.isNotEmpty()) Row(
                    Modifier.fillMaxWidth().background(Pine).padding(horizontal = 22.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text("${state.staged.size} pending ${if (state.staged.size == 1) "change" else "changes"}", color = Color.White, fontWeight = FontWeight.SemiBold)
                        Text("Review before saving", color = Color(0xFFB7C8BE), fontSize = 12.sp)
                    }
                    IconButton(onClick = { discard = true }, enabled = !busy) {
                        Icon(Icons.AutoMirrored.Filled.Undo, "Discard staged changes", tint = Color.White)
                    }
                    Button(onClick = { review = true }, enabled = !busy,
                        colors = ButtonDefaults.buttonColors(containerColor = Lime, contentColor = Pine)) {
                        Text("Review"); Spacer(Modifier.width(7.dp)); Icon(Icons.AutoMirrored.Filled.ArrowForward, null, Modifier.size(17.dp))
                    }
                }
                NavigationBar(containerColor = Color.White, tonalElevation = 0.dp) {
                    listOf("Overview" to Icons.Default.Dashboard, "Settings" to Icons.Default.Tune, "Backups" to Icons.Default.History)
                        .forEachIndexed { index, (label, icon) ->
                            NavigationBarItem(selected = page == index, onClick = { page = index },
                                icon = { Icon(icon, label) }, label = { Text(label) },
                                colors = NavigationBarItemDefaults.colors(indicatorColor = Lime, selectedIconColor = Pine))
                        }
                }
            }
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (demo && connected) Row(Modifier.fillMaxWidth().background(Color(0xFFF0E8D0)).padding(12.dp),
                horizontalArrangement = Arrangement.Center) {
                Text("DEMO BIKE  ·  Changes stay in memory", fontSize = 12.sp, color = Color(0xFF735E27))
            }
            if (busy) {
                LinearProgressIndicator(Modifier.fillMaxWidth(), color = Pine, trackColor = Lime)
                Row(Modifier.padding(horizontal = 24.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(state.status, Modifier.weight(1f), color = Muted, fontSize = 13.sp)
                    if (state.phase == AppPhase.Connecting) TextButton(onClick = vm::cancelConnection) { Text("Cancel") }
                }
            }
            val message = state.error ?: notice
            if (message != null) Card(
                Modifier.padding(horizontal = 20.dp, vertical = 8.dp).fillMaxWidth(),
                colors = CardDefaults.cardColors(containerColor = Color(0xFFF8E8DE)),
            ) {
                Row(Modifier.padding(start = 16.dp, top = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text(message, color = Color(0xFF78452A), fontSize = 13.sp)
                        if (state.error != null && state.phase == AppPhase.Disconnected)
                            Text(state.status, color = Color(0xFF78452A), fontSize = 12.sp)
                    }
                    IconButton(onClick = { vm.controller.dismissError(); vm.showNotice(null) }) { Icon(Icons.Default.Close, "Dismiss message") }
                }
            }
            when {
                page == 2 -> BackupsScreen(backups, backupInfo, state.identity, connected && !busy && !demo && state.staged.isEmpty(),
                    onImport = { importLauncher.launch(arrayOf("application/json", "text/plain")) },
                    onExport = { file -> exportFile = file; exportLauncher.launch(file) },
                    onRestore = { restoreFile = it }, onBack = { page = 0 })
                !connected -> WelcomeScreen(busy, onScan = { scan() }, onDemo = { page = 1; vm.startDemo() }, onBackups = { page = 2 })
                page == 0 -> OverviewScreen(state, demo, !busy, onTune = { page = 1 }, onRefresh = vm::refresh, onStage = vm.controller::stage)
                else -> TuneScreen(state, enabled = !busy, onStage = vm.controller::stage)
            }
        }
    }

    if (showScan) AlertDialog(
        onDismissRequest = { showScan = false; vm.stopScan() },
        icon = { Icon(Icons.AutoMirrored.Filled.BluetoothSearching, null, tint = Pine) },
        title = { Text("Scan for STEPS bikes") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("Power on the bike and activate Bluetooth connection mode on the display. Close other bike apps.")
                if (scanning) LinearProgressIndicator(Modifier.fillMaxWidth())
                if (nearby.isEmpty()) Text(if (scanning) "Looking for E-TUBE gateways…" else "No gateways found. Activate connection mode and scan again.", color = Muted)
                nearby.forEach { bike ->
                    OutlinedCard(onClick = { vm.stopScan(); showScan = false; selectedBike = bike }, modifier = Modifier.fillMaxWidth()) {
                        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                            Icon(Icons.Default.ElectricBike, null, tint = Pine)
                            Column(Modifier.weight(1f).padding(start = 12.dp)) {
                                Text(bike.name, fontWeight = FontWeight.SemiBold)
                                Text("${bike.rssi} dBm  ·  ${bike.address}", fontSize = 11.sp, color = Muted)
                            }
                            Icon(Icons.Default.ChevronRight, "Connect")
                        }
                    }
                }
                notice?.let { Text(it, color = MaterialTheme.colorScheme.error, fontSize = 13.sp) }
            }
        },
        confirmButton = { TextButton(onClick = { scan() }, enabled = !scanning) { Text("Scan again") } },
        dismissButton = { TextButton(onClick = { showScan = false; vm.stopScan() }) { Text("Close") } },
    )
    selectedBike?.let { bike ->
        PasskeyDialog(bike.name, onDismiss = { selectedBike = null }) { pin ->
            selectedBike = null
            page = 0
            vm.connect(bike, pin)
        }
    }
    if (review) AlertDialog(
        onDismissRequest = { if (!busy) review = false },
        title = { Text(if (demo) "Review demo changes" else "Save to your bike?") },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(14.dp)) {
                Text(if (demo) "These changes affect the demo bike only." else "Each change is backed up, written in order, and checked by reading it back. Keep the bike powered and nearby.")
                state.staged.forEach { (name, value) ->
                    val setting = state.settings.find { it.name == name }
                    Column {
                        Text(setting?.label ?: settingPresentation(name).label, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                        Text("${formatSettingValue(name, state.values[name] ?: "—", state.identity?.modelSlug)}  →  ${formatSettingValue(name, value, state.identity?.modelSlug)}", color = Pine, fontFamily = FontFamily.Monospace)
                    }
                }
                if (state.staged.keys.any { it == "destination" || it == "speed-limit" })
                    Text("Choose settings that comply with the rules where you ride.", color = MaterialTheme.colorScheme.error, fontSize = 12.sp)
                if (!demo) Text("A failed save can leave earlier changes applied. Reconnect and read the bike before restoring a backup.", fontSize = 12.sp, color = Muted)
            }
        },
        confirmButton = { Button(onClick = { review = false; vm.commit() }, enabled = !busy && state.staged.isNotEmpty()) { Text(if (demo) "Apply in demo" else "Back up & save") } },
        dismissButton = { TextButton(onClick = { review = false }) { Text("Keep editing") } },
    )
    if (disconnect) AlertDialog(
        onDismissRequest = { disconnect = false }, title = { Text("Disconnect bike?") },
        text = { Text(if (state.staged.isEmpty()) "Saved settings remain on the bike." else "${state.staged.size} pending ${if (state.staged.size == 1) "change will" else "changes will"} be discarded.") },
        confirmButton = { TextButton(onClick = { disconnect = false; vm.controller.discard(); vm.disconnect(); page = 0 }) { Text("Disconnect") } },
        dismissButton = { TextButton(onClick = { disconnect = false }) { Text("Cancel") } },
    )
    if (discard) AlertDialog(
        onDismissRequest = { discard = false }, title = { Text("Discard staged changes?") },
        text = { Text("Pending edits will be cleared. Saved bike settings are unaffected.") },
        confirmButton = { TextButton(onClick = { discard = false; vm.controller.discard() }) { Text("Discard") } },
        dismissButton = { TextButton(onClick = { discard = false }) { Text("Keep editing") } },
    )
    restoreFile?.let { file -> AlertDialog(
        onDismissRequest = { restoreFile = null }, title = { Text("Restore this setting?") },
        text = {
            val info = backupInfo[file]
            Text("${info?.let { settingPresentation(it.setting, it.model).label } ?: file}\nRestore value: ${info?.let { formatSettingValue(it.setting, it.before, it.model) } ?: "Unknown"}\nMotor: ${info?.model ?: "Unknown"}\nSerial: ${info?.serial ?: "Unknown"}\n\nThe current value is backed up before restore and verified afterward.")
        },
        confirmButton = { Button(onClick = { restoreFile = null; vm.restore(file) },
            enabled = !busy && !demo && state.staged.isEmpty() && backupInfo[file]?.matches(state.identity) == true) { Text("Restore & verify") } },
        dismissButton = { TextButton(onClick = { restoreFile = null }) { Text("Cancel") } },
    ) }
}

@Composable
private fun WelcomeScreen(busy: Boolean, onScan: () -> Unit, onDemo: () -> Unit, onBackups: () -> Unit) {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp), verticalArrangement = Arrangement.spacedBy(22.dp)) {
        PageTitle("Connect to a bike", "Read and change Shimano STEPS drive-unit settings over Bluetooth LE.")
        Text("Turn on the bike and enable Bluetooth connection mode on its display. Close any other app connected to the bike.", color = Muted, fontSize = 14.sp, lineHeight = 22.sp)
        Button(onClick = onScan, enabled = !busy, modifier = Modifier.fillMaxWidth().height(56.dp), shape = RoundedCornerShape(16.dp)) {
            Icon(Icons.AutoMirrored.Filled.BluetoothSearching, null); Spacer(Modifier.width(10.dp)); Text("Scan for bikes", fontSize = 16.sp)
        }
        OutlinedButton(onClick = onDemo, enabled = !busy, modifier = Modifier.fillMaxWidth().height(52.dp), shape = RoundedCornerShape(16.dp)) {
            Text("Open demo")
        }
        Text("The demo uses sample settings and does not connect to a bike.", color = Muted, fontSize = 13.sp, lineHeight = 20.sp)
        TextButton(onClick = onBackups, modifier = Modifier.align(Alignment.CenterHorizontally)) { Text("View backups") }
        Text("Experimental software. Changing settings can damage components or void warranties. Available settings depend on the motor model and firmware.", color = Muted, fontSize = 12.sp, lineHeight = 18.sp)
    }
}

@Composable
private fun OverviewScreen(state: AppState, demo: Boolean, enabled: Boolean, onTune: () -> Unit, onRefresh: () -> Unit, onStage: (String, String) -> Unit) {
    val identity = state.identity ?: return
    var editing by remember { mutableStateOf<TuningSetting?>(null) }
    fun editable(name: String) = state.settings.find { it.name == name }
        ?.takeIf { enabled && it.writable && name in state.values && name !in state.unavailable }
    Column(Modifier.verticalScroll(rememberScrollState()).padding(24.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
        PageTitle("Bike overview", if (demo) "Sample drive-unit identity and settings." else "Drive-unit identity and settings read from the bike.")
        Card(colors = CardDefaults.cardColors(containerColor = Pine), shape = RoundedCornerShape(26.dp)) {
            Column(Modifier.fillMaxWidth().padding(24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("SHIMANO STEPS", color = Color(0xFFADC3B4), letterSpacing = 2.sp, fontSize = 10.sp, modifier = Modifier.weight(1f))
                    Icon(Icons.Default.ElectricBike, null, tint = Lime, modifier = Modifier.size(34.dp))
                }
                Text(identity.model, color = Color.White, fontSize = 29.sp, fontWeight = FontWeight.SemiBold)
                Row(horizontalArrangement = Arrangement.spacedBy(24.dp)) {
                    MotorDatum("SERIAL", identity.serial)
                    MotorDatum("LINK", if (demo) "Demo" else "Bluetooth LE")
                }
                HorizontalDivider(color = Color(0xFF3D6251))
                Text("${state.values.size} settings read  ·  ${state.unavailable.size} unavailable", color = Lime, fontSize = 12.sp)
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            listOf(Triple("Assist cut-off", "speed-limit", "km/h"), Triple("Wheel rollout", "tire-circumference", "mm")).forEach { (label, name, unit) ->
                val setting = editable(name)
                MetricCard(label, state.staged[name] ?: state.values[name] ?: "—", unit, Modifier.weight(1f),
                    onClick = setting?.let { { editing = it } })
            }
        }
        Card(onClick = onTune, shape = RoundedCornerShape(20.dp), colors = CardDefaults.cardColors(containerColor = Lime)) {
            Row(Modifier.fillMaxWidth().padding(22.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Default.Tune, null, tint = Pine)
                Column(Modifier.weight(1f).padding(horizontal = 16.dp)) {
                    Text("Edit settings", fontWeight = FontWeight.Bold, color = Pine)
                    Text("Assist profiles, wheel size, lights and service", fontSize = 12.sp, color = Pine)
                }
                Icon(Icons.AutoMirrored.Filled.ArrowForward, "Open settings", tint = Pine)
            }
        }
        Card(shape = RoundedCornerShape(20.dp)) {
            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
                Text("Drive unit details", fontWeight = FontWeight.SemiBold)
                DetailRow("Application version", identity.dcasVersion)
                DetailRow("Motor controller version", identity.rxVersion)
                DetailRow("Bus slot", identity.slot.toString())
            }
        }
        OutlinedButton(onClick = onRefresh, enabled = enabled && state.staged.isEmpty(), modifier = Modifier.fillMaxWidth()) {
            Icon(Icons.Default.Refresh, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text("Read settings again")
        }
        Text(state.status, color = Muted, fontSize = 12.sp)
    }
    editing?.let { SettingEditor(it, state, onStage) { editing = null } }
}

@Composable
private fun MotorDatum(title: String, value: String) {
    Column(verticalArrangement = Arrangement.spacedBy(5.dp)) {
        Text(title, color = Color(0xFFADC3B4), fontSize = 9.sp, letterSpacing = 1.sp)
        Text(value, color = Color.White, fontSize = 12.sp, fontFamily = FontFamily.Monospace)
    }
}

@Composable
private fun MetricCard(label: String, value: String, unit: String, modifier: Modifier, onClick: (() -> Unit)? = null) {
    val shape = RoundedCornerShape(20.dp)
    Card(modifier.clip(shape).clickable(enabled = onClick != null, onClickLabel = "Edit $label") { onClick?.invoke() }, shape = shape) {
        Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(label, color = Muted, fontSize = 12.sp, modifier = Modifier.weight(1f))
                if (onClick != null) Icon(Icons.Default.ChevronRight, null, tint = Muted, modifier = Modifier.size(17.dp))
            }
            Text(value.removeSuffix(".0"), fontSize = 30.sp, fontWeight = FontWeight.SemiBold, letterSpacing = (-1).sp)
            Text(unit, color = Muted, fontSize = 11.sp)
        }
    }
}

@Composable
private fun DetailRow(label: String, value: String) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
        Text(label, color = Muted, fontSize = 12.sp)
        Text(value, fontSize = 12.sp, fontFamily = FontFamily.Monospace)
    }
}

@Composable
private fun TuneScreen(state: AppState, enabled: Boolean, onStage: (String, String) -> Unit) {
    var section by rememberSaveable { mutableStateOf("profiles") }
    var profile by rememberSaveable { mutableIntStateOf(1) }
    var editing by remember { mutableStateOf<TuningSetting?>(null) }
    val sections = listOf("profiles" to "Assist", "geometry" to "Bike", "maintenance" to "Service", "oem" to "Factory")
    val filtered = state.settings.filter {
        it.section == section && (section != "profiles" || !it.name.startsWith("profile") || it.name.startsWith("profile$profile."))
            && !(section == "maintenance" && state.settings.any { spec -> spec.name == "maintenance-date" }
                && it.name in setOf("maintenance-year", "maintenance-month", "maintenance-day"))
    }
    LazyColumn(contentPadding = PaddingValues(24.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
        item { PageTitle("Settings", "Read current values and review changes before saving.") }
        item {
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                sections.forEach { (id, label) -> FilterChip(selected = section == id, onClick = { section = id }, label = { Text(label) }) }
            }
        }
        if (section == "profiles") {
            item {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    (1..2).forEach { number ->
                        FilterChip(selected = profile == number, onClick = { profile = number }, label = { Text("Profile $number") },
                            leadingIcon = { Icon(Icons.Default.Landscape, null, Modifier.size(17.dp)) })
                    }
                }
            }
            item {
                Text("Assist profiles", fontSize = 21.sp, fontWeight = FontWeight.SemiBold)
                Text("Set peak torque, character and start response for each mode.", fontSize = 13.sp, color = Muted)
            }
        }
        if (section == "oem") item {
            NoticeCard("Factory settings", "Installation settings and factory limits affect motor operation. Only change values you understand.")
        }
        if (filtered.isEmpty()) item {
            NoticeCard("No settings in this category", "This motor or firmware does not expose these controls.")
        }
        if (section == "profiles") {
            listOf("eco" to "Eco", "trail" to "Trail", "boost" to "Boost").forEach { (mode, title) ->
                val modeSettings = filtered.filter { it.name.startsWith("profile$profile.$mode.") }
                if (modeSettings.isNotEmpty()) item {
                    Card(shape = RoundedCornerShape(22.dp)) {
                        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Box(Modifier.size(36.dp).clip(RoundedCornerShape(12.dp)).background(
                                    when (mode) { "eco" -> Color(0xFFE6F0DB); "trail" -> Color(0xFFE6EEDC); else -> Color(0xFFF4E9D4) }), contentAlignment = Alignment.Center) {
                                    Icon(if (mode == "eco") Icons.Default.Eco else if (mode == "trail") Icons.Default.Landscape else Icons.Default.Bolt, null, tint = Pine, modifier = Modifier.size(21.dp))
                                }
                                Text(title, Modifier.padding(start = 12.dp), fontSize = 19.sp, fontWeight = FontWeight.SemiBold)
                            }
                            modeSettings.forEach { spec ->
                                SettingRow(spec, state, enabled, compact = true, onClick = { editing = spec })
                            }
                        }
                    }
                }
            }
            items(filtered.filter { !it.name.matches(Regex("profile[12]\\.(eco|trail|boost)\\..*")) }, key = { it.name }) { spec ->
                SettingCard(spec, state, enabled) { editing = spec }
            }
        } else items(filtered, key = { it.name }) { spec -> SettingCard(spec, state, enabled) { editing = spec } }
        item { Text("Edits are staged until you review and save them.", color = Muted, fontSize = 12.sp, modifier = Modifier.padding(vertical = 12.dp)) }
    }
    editing?.let { SettingEditor(it, state, onStage) { editing = null } }
}

@Composable
private fun SettingEditor(setting: TuningSetting, state: AppState, onStage: (String, String) -> Unit, onDone: () -> Unit) {
    EditSettingDialog(setting, state.staged[setting.name] ?: state.values[setting.name] ?: "", state.identity?.modelSlug,
        onDismiss = onDone, onStage = { value -> onStage(setting.name, value); onDone() },
        onUndo = if (setting.name in state.staged) { { onStage(setting.name, state.values[setting.name] ?: ""); onDone() } } else null)
}

@Composable
private fun SettingCard(setting: TuningSetting, state: AppState, enabled: Boolean, onClick: () -> Unit) {
    Card(shape = RoundedCornerShape(18.dp)) { Box(Modifier.padding(horizontal = 18.dp, vertical = 3.dp)) { SettingRow(setting, state, enabled, false, onClick) } }
}

@Composable
private fun SettingRow(setting: TuningSetting, state: AppState, enabled: Boolean, compact: Boolean, onClick: () -> Unit) {
    val available = setting.name in state.values && setting.name !in state.unavailable
    val changed = setting.name in state.staged
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp))
        .clickable(enabled = enabled && available && setting.writable, onClick = onClick).padding(vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f).padding(end = 12.dp)) {
            val label = if (compact) when { setting.name.endsWith(".torque") -> "Peak torque"; setting.name.endsWith(".character") -> "Assist character"; else -> "Start response" } else setting.label
            Text(label, fontSize = 14.sp, fontWeight = if (compact) FontWeight.Normal else FontWeight.Medium)
            if (setting.description.isNotBlank()) Text(setting.description, color = Muted, fontSize = 12.sp,
                lineHeight = 17.sp, modifier = Modifier.padding(top = 4.dp))
            if (!available) Text(state.unavailable[setting.name] ?: "Not available", color = Muted, fontSize = 11.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
            else if (changed) Text("Bike: ${displayValue(setting, state.values[setting.name] ?: "—", state.identity?.modelSlug)}", color = Muted, fontSize = 11.sp)
            else if (!setting.writable) Text("Read only", color = Muted, fontSize = 11.sp)
        }
        val value = state.staged[setting.name] ?: state.values[setting.name] ?: "—"
        Text(displayValue(setting, value, state.identity?.modelSlug), fontWeight = FontWeight.SemiBold, fontSize = 14.sp, color = if (changed) Color(0xFF47761C) else Pine)
        if (changed) Icon(Icons.Default.Circle, "Staged", tint = Color(0xFF73A93F), modifier = Modifier.padding(start = 7.dp).size(6.dp))
        if (setting.writable && available) Icon(Icons.Default.ChevronRight, "Edit ${setting.label}", tint = Muted, modifier = Modifier.padding(start = 7.dp).size(19.dp))
    }
}

private fun displayValue(setting: TuningSetting, value: String, model: String?): String =
    formatSettingValue(setting.name, value, model)

@Composable
private fun EditSettingDialog(setting: TuningSetting, current: String, model: String?, onDismiss: () -> Unit, onStage: (String) -> Unit, onUndo: (() -> Unit)?) {
    var value by remember(setting.name) { mutableStateOf(setting.inputText(current)) }
    var inputError by remember(setting.name) { mutableStateOf<String?>(null) }
    AlertDialog(onDismissRequest = onDismiss, title = { Text(setting.label) }, text = {
        Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            if (setting.description.isNotBlank()) Text(setting.description, fontSize = 14.sp, lineHeight = 21.sp)
            if (setting.choices.isNotEmpty()) setting.choices.forEach { choice ->
                OutlinedButton(onClick = { value = setting.inputText(choice); inputError = null }, modifier = Modifier.fillMaxWidth(),
                    colors = ButtonDefaults.outlinedButtonColors(containerColor = if (value == setting.inputText(choice)) Lime else Color.Transparent)) {
                    Text(displayValue(setting, choice, model))
                }
            } else {
                OutlinedTextField(value = value, onValueChange = { value = it; inputError = null }, label = { Text(if (setting.unit.isBlank()) "Value" else "Value · ${setting.unit}") },
                    singleLine = true, modifier = Modifier.fillMaxWidth(),
                    isError = inputError != null,
                    keyboardOptions = KeyboardOptions(keyboardType = if (setting.valueType == "number" && setting.displayMin >= 0) KeyboardType.Decimal else KeyboardType.Text))
                if (setting.valueType == "number") {
                    val suffix = if (setting.unit.isBlank()) "" else if (setting.unit in listOf("%", "°")) setting.unit else " ${setting.unit}"
                    Text("Range ${formatInput(setting.displayMin)}–${formatInput(setting.displayMax)}$suffix · increments of ${formatInput(setting.displayStep)}$suffix", fontSize = 12.sp, color = Muted)
                    if (setting.displayMax > setting.displayMin && value.toFloatOrNull() != null) Slider(
                        value = value.toFloat().coerceIn(setting.displayMin.toFloat(), setting.displayMax.toFloat()),
                        onValueChange = { next ->
                            val steps = kotlin.math.round((next - setting.displayMin) / setting.displayStep)
                            value = formatInput((setting.displayMin + steps * setting.displayStep).coerceIn(setting.displayMin, setting.displayMax))
                            inputError = null
                        }, valueRange = setting.displayMin.toFloat()..setting.displayMax.toFloat())
                }
            }
            inputError?.let { Text(it, color = MaterialTheme.colorScheme.error, fontSize = 12.sp) }
            Text("This updates your staged changes. The bike is written only after review.", color = Muted, fontSize = 12.sp)
        }
    }, confirmButton = { Button(onClick = {
        try { onStage(setting.backendInput(value)) }
        catch (error: IllegalArgumentException) { inputError = error.message ?: "Enter a supported value." }
    }, enabled = value.isNotBlank() || setting.valueType == "string") { Text("Stage change") } },
        dismissButton = { Row { if (onUndo != null) TextButton(onClick = onUndo) { Text("Undo") }; TextButton(onClick = onDismiss) { Text("Cancel") } } })
}

private fun formatInput(value: Double): String = if (value == value.toLong().toDouble()) value.toLong().toString() else "%.2f".format(java.util.Locale.ROOT, value).trimEnd('0').trimEnd('.')

@Composable
private fun BackupsScreen(files: List<String>, metadata: Map<String, BackupInfo>, identity: MotorIdentity?, canRestore: Boolean, onImport: () -> Unit, onExport: (String) -> Unit, onRestore: (String) -> Unit, onBack: () -> Unit) {
    LazyColumn(contentPadding = PaddingValues(24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        item { PageTitle("Backups", "Restore previous setting values or export a copy.") }
        item { NoticeCard("Before every write", "The original setting is stored on this phone, bound to the motor’s model and serial. Export backups to keep a separate copy.") }
        item { OutlinedButton(onClick = onImport, modifier = Modifier.fillMaxWidth()) { Icon(Icons.Default.FileDownload, null, Modifier.size(18.dp)); Spacer(Modifier.width(8.dp)); Text("Import a backup") } }
        if (files.isEmpty()) item {
            Column(Modifier.fillMaxWidth().padding(vertical = 34.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Icon(Icons.Default.History, null, tint = Muted, modifier = Modifier.size(44.dp))
                Text("No backups yet", fontWeight = FontWeight.SemiBold, fontSize = 19.sp)
                Text("A backup is created before each setting is saved.", color = Muted, fontSize = 13.sp)
            }
        }
        items(files, key = { it }) { file -> Card(shape = RoundedCornerShape(18.dp)) {
            val info = metadata[file]
            Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Default.Description, null, tint = Pine)
                    Text(file, Modifier.padding(start = 10.dp), fontFamily = FontFamily.Monospace, fontSize = 11.sp, maxLines = 3)
                }
                if (info != null) {
                    Text(settingPresentation(info.setting, info.model).label, fontWeight = FontWeight.SemiBold, fontSize = 14.sp)
                    Text("Original: ${formatSettingValue(info.setting, info.before, info.model)}\n${info.model} · ${info.serial}", color = Muted, fontSize = 12.sp)
                } else Text("Backup details unavailable. Export to inspect the file.", color = Muted, fontSize = 12.sp)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    TextButton(onClick = { onExport(file) }) { Icon(Icons.Default.FileUpload, null, Modifier.size(16.dp)); Spacer(Modifier.width(7.dp)); Text("Export") }
                    OutlinedButton(onClick = { onRestore(file) }, enabled = canRestore && info?.matches(identity) == true) { Text("Restore") }
                }
            }
        } }
        if (!canRestore && files.isNotEmpty()) item { Text("Connect your bike and save or discard staged changes to restore a backup.", color = Muted, fontSize = 12.sp) }
        item { TextButton(onClick = onBack) { Text("Back to bike") } }
    }
}

private fun BackupInfo.matches(identity: MotorIdentity?): Boolean = identity != null &&
    serial.equals(identity.serial, ignoreCase = true) && model == identity.modelSlug

@Composable
private fun PasskeyDialog(name: String, onDismiss: () -> Unit, onConnect: (String) -> Unit) {
    var passkey by remember { mutableStateOf("") }
    AlertDialog(onDismissRequest = onDismiss, icon = { Icon(Icons.Default.Lock, null, tint = Pine) }, title = { Text("Your bike’s passkey") },
        text = { Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text("Enter the six-digit E-TUBE passkey for $name. It is used for this connection and isn’t saved.")
            OutlinedTextField(passkey, { passkey = it.filter { char -> char in '0'..'9' }.take(6) }, label = { Text("Six-digit passkey") }, singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword), visualTransformation = PasswordVisualTransformation())
        } }, confirmButton = { Button(onClick = { onConnect(passkey); passkey = "" }, enabled = passkey.length == 6) { Text("Connect") } },
        dismissButton = { TextButton(onClick = { passkey = ""; onDismiss() }) { Text("Cancel") } })
}

@Composable
private fun PageTitle(title: String, subtitle: String) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(title, fontSize = 34.sp, fontWeight = FontWeight.SemiBold, letterSpacing = (-1.2).sp)
        Text(subtitle, fontSize = 14.sp, color = Muted)
    }
}

@Composable
private fun NoticeCard(title: String, message: String) {
    Card(shape = RoundedCornerShape(18.dp), colors = CardDefaults.cardColors(containerColor = Color(0xFFE8EDDF))) {
        Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(7.dp)) {
            Text(title, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, color = Pine)
            Text(message, fontSize = 12.sp, color = Muted, lineHeight = 18.sp)
        }
    }
}

@Composable
private fun StatusPill(label: String, color: Color) {
    Row(Modifier.clip(RoundedCornerShape(40.dp)).background(color).padding(horizontal = 10.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
        Box(Modifier.size(5.dp).background(Pine, CircleShape))
        Text(label, fontSize = 9.sp, fontWeight = FontWeight.Bold, color = Pine, letterSpacing = 1.sp)
    }
}
