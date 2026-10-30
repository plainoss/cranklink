package work.plainoss.cranklink

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import work.plainoss.cranklink.shared.settingPresentation
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

@RunWith(AndroidJUnit4::class)
class AppSmokeTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun systemBackConfirmsDisconnectWithoutLosingPendingEdits() {
        ui.onNodeWithText("Open demo").performScrollTo().performClick()
        ui.waitUntil(5_000) { ui.onAllNodesWithText("Market region").fetchSemanticsNodes().isNotEmpty() }
        ui.onNodeWithText("Assist").performClick()
        ui.onAllNodesWithText("Peak torque").onFirst().performClick()
        ui.onNode(hasSetTextAction()).performTextClearance()
        ui.onNode(hasSetTextAction()).performTextInput("55")
        ui.onNodeWithText("Stage change").performClick()
        ui.runOnIdle { ui.activity.onBackPressedDispatcher.onBackPressed() }
        ui.onNodeWithText("Disconnect bike?").assertIsDisplayed()
        ui.onNodeWithText("1 pending change will be discarded.").assertIsDisplayed()
        ui.onNodeWithText("Cancel").performClick()
        ui.onNodeWithText("1 pending change").assertIsDisplayed()
        assertFalse(ui.activity.isFinishing)
        ui.runOnIdle { ui.activity.onBackPressedDispatcher.onBackPressed() }
        ui.onNodeWithText("Disconnect", useUnmergedTree = true).performClick()
        ui.waitUntil(5_000) { ui.onAllNodesWithText("Open demo").fetchSemanticsNodes().isNotEmpty() }
        assertFalse(ui.activity.isFinishing)
    }

    @Test fun explicitlyClosedGattCannotBeRevivedByAQueuedConnect() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val gatt = AndroidGatt(context, "00:00:00:00:00:00", "000000")
        gatt.close()
        val worker = Executors.newSingleThreadExecutor()
        try {
            for (operation in listOf({ gatt.open() }, { gatt.reopen() })) {
                val failure = worker.submit<Throwable?> { runCatching(operation).exceptionOrNull() }
                    .get(2, TimeUnit.SECONDS)
                assertTrue("An explicit close must fence queued connects before Bluetooth lookup", failure is IllegalStateException)
                assertTrue(failure?.message.orEmpty().contains("closed"))
            }
        } finally {
            worker.shutdownNow()
            gatt.close()
        }
    }

    @Test fun demoStagesReviewsAndAppliesWithoutBluetoothPermission() {
        ui.onNodeWithText("Open demo").performScrollTo().performClick()
        ui.waitUntil(5_000) { ui.onAllNodesWithText("Market region").fetchSemanticsNodes().isNotEmpty() }
        ui.onNodeWithText("Assist").performClick()
        ui.onAllNodesWithText("Peak torque").onFirst().performClick()
        ui.onNode(hasSetTextAction()).performTextClearance()
        ui.onNode(hasSetTextAction()).performTextInput("55")
        ui.onNodeWithText("Stage change").performClick()
        ui.onNodeWithText("1 pending change").assertIsDisplayed()
        ui.onNodeWithText("Review").performClick()
        ui.onNodeWithText("Review demo changes").assertIsDisplayed()
        ui.onNodeWithText("Apply in demo").performClick()
        ui.waitUntil(5_000) { ui.onAllNodesWithText("1 pending change").fetchSemanticsNodes().isEmpty() }
        ui.onNodeWithText("55 Nm").assertIsDisplayed()
        ui.onNodeWithText("DEMO BIKE  ·  Changes stay in memory").assertIsDisplayed()
    }

    @Test fun demoEditsPhysicalWattsAndKeepsUnitsThroughReviewAndReadback() {
        ui.onNodeWithText("Open demo").performScrollTo().performClick()
        ui.waitUntil(5_000) { ui.onAllNodesWithText("Market region").fetchSemanticsNodes().isNotEmpty() }

        // The demo stores calibration as 15 tenths of a percent. Its card must
        // show the physical percentage, not the canonical backend number.
        ui.onNode(hasScrollToNodeAction()).performScrollToNode(hasText("1.5%"))
        ui.onNodeWithText("1.5%").assertIsDisplayed()
        ui.onNode(hasScrollToNodeAction()).performScrollToNode(hasText("13.5 W"))
        ui.onNodeWithText("13.5 W").assertIsDisplayed()
        val description = settingPresentation("light-output").description
        assertTrue("Light power needs an operator-facing description", description.isNotBlank())
        ui.onNodeWithText(description).assertIsDisplayed()
        ui.onNodeWithText("13.5 W").performClick()

        // Canonical 1350 must become an editable 13.5 W. A fractional-watt
        // change exercises conversion back to the integer backend encoding.
        ui.onNode(hasSetTextAction()).assertTextContains("13.5")
        ui.onNodeWithText("Range 1.35–27 W · increments of 0.01 W").assertIsDisplayed()
        ui.onNode(hasSetTextAction()).performTextClearance()
        ui.onNode(hasSetTextAction()).performTextInput("13.75")
        ui.onNodeWithText("Stage change").performClick()
        ui.onNodeWithText("1 pending change").assertIsDisplayed()
        ui.onNodeWithText("13.75 W").assertIsDisplayed()
        ui.onNodeWithText("Review").performClick()
        ui.onNodeWithText("Review demo changes").assertIsDisplayed()
        ui.onNodeWithText("13.5 W  →  13.75 W").assertIsDisplayed()
        ui.onNodeWithText("Apply in demo").performClick()

        ui.waitUntil(5_000) { ui.onAllNodesWithText("1 pending change").fetchSemanticsNodes().isEmpty() }
        ui.onNode(hasScrollToNodeAction()).performScrollToNode(hasText("13.75 W"))
        ui.onNodeWithText("13.75 W").assertIsDisplayed().performClick()
        // Reopening after applying reads the saved demo snapshot. This catches
        // double scaling and accidental storage of the display number itself.
        ui.onNode(hasSetTextAction()).assertTextContains("13.75")
        ui.onNodeWithText("Cancel").performClick()
        ui.onNodeWithText("DEMO BIKE  ·  Changes stay in memory").assertIsDisplayed()
    }

    @Test fun packagedNativeCoreLoadsAndRejectsOutOfScopeOperations() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val directory = java.io.File(context.cacheDir, "jni-smoke-backups").apply { mkdirs() }
        val gatt = AndroidGatt(context, "00:00:00:00:00:00", "000000")
        NativeCore.ensureLoaded()
        val backups = Json.parseToJsonElement(NativeCore.invoke(gatt, "{\"op\":\"backups\"}", directory.absolutePath)).jsonObject
        assertTrue(backups.containsKey("result"))
        for (request in listOf("{\"op\":\"firmware\"}", "{\"op\":\"connect\",\"component\":\"RD-R8150\"}")) {
            val rejected = Json.parseToJsonElement(NativeCore.invoke(gatt, request, directory.absolutePath)).jsonObject
            assertTrue(rejected.containsKey("error"))
        }
        val disconnected = NativeCore.invoke(gatt, "{\"op\":\"connect\"}", directory.absolutePath)
        assertTrue(disconnected, Json.parseToJsonElement(disconnected).jsonObject.containsKey("error"))
        assertFalse(disconnected, disconnected.contains("NoSuchMethod"))
        gatt.close()
    }
}
