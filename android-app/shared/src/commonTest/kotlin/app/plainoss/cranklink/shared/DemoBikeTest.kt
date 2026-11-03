package app.plainoss.cranklink.shared

import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class DemoBikeTest {
    private suspend fun connected(): SessionController = SessionController(DemoBike()).also { it.connect() }

    @Test
    fun demoIsAFullyWritableEp800OnABluetoothMasterSession() = runTest {
        val state = connected().state.value
        assertEquals(AppPhase.Ready, state.phase, state.error)
        assertTrue(state.identity!!.masterRouting)
        assertTrue(state.settings.all { it.writable }, state.settings.filterNot { it.writable }.joinToString { it.name })
        assertTrue(state.settings.any { it.section == "oem" }, "OEM Factory Bounds must open in the demo")
        assertEquals("Europe · max 25 km/h", formatSettingValue("destination", state.values["destination"]!!, "ep800-crg"))
        // The real EP800 Bluetooth schema: whole km/h from 15 to 32.
        val speed = state.settings.single { it.name == "speed-limit" }
        assertEquals(listOf(15.0, 32.0, 1.0), listOf(speed.displayMin, speed.displayMax, speed.displayStep))
    }

    @Test
    fun demoChangesRegionAndSpeedToTheRegionMaximum() = runTest {
        val controller = connected()
        controller.stage("speed-limit", "32")
        assertNull(controller.state.value.error)
        assertEquals(mapOf("speed-limit" to "32", "destination" to "1"), controller.state.value.staged)
        assertEquals(32.0, controller.state.value.reachableSpeedMax())
        controller.commit()
        assertNull(controller.state.value.error)
        assertEquals("1", controller.state.value.values["destination"])
        assertEquals("32", controller.state.value.values["speed-limit"])
        controller.stage("tire-circumference", "2200")
        assertEquals(mapOf("tire-circumference" to "2200"), controller.state.value.staged)
    }

    @Test
    fun demoBacksUpEachSaveAndRestoresIt() = runTest {
        val bike = DemoBike()
        val controller = SessionController(bike)
        controller.connect()
        controller.stage("tire-circumference", "2200")
        controller.commit()
        val backup = bike.backups.single()
        assertEquals("tire-circumference" to "2288", backup.setting to backup.before)
        assertEquals(listOf(backup.file), controller.state.value.backups)
        controller.restore(backup.file)
        assertNull(controller.state.value.error)
        assertEquals("2288", controller.state.value.values["tire-circumference"])
        assertEquals(2, bike.backups.size)
    }

    @Test
    fun demoRefusesARestoreThatLeavesSpeedAboveTheRegion() = runTest {
        val bike = DemoBike()
        val controller = SessionController(bike)
        controller.connect()
        controller.stage("speed-limit", "32")
        controller.commit()
        val region = bike.backups.single { it.setting == "destination" }
        controller.restore(region.file)
        assertEquals("Europe · max 25 km/h allows 15–25 km/h, but assist speed would be 32 km/h. Change the assist speed first.",
            controller.state.value.error)
        controller.connect()
        assertEquals("1", controller.state.value.values["destination"])
    }

    @Test
    fun previewNamesTheRegionAFasterSpeedNeedsWithoutStaging() = runTest {
        val controller = connected()
        val note = controller.preview("speed-limit", "30").getOrThrow()
        assertEquals("Market region changes to United States · max 32 km/h to allow 30 km/h.", note)
        assertTrue(controller.state.value.staged.isEmpty())
    }
}
