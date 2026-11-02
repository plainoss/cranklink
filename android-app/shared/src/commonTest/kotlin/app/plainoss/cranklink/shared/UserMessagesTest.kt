package app.plainoss.cranklink.shared

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class UserMessagesTest {
    @Test
    fun knownBluetoothFailuresUseRiderWording() {
        assertTrue(userFacingError("BLE authentication failed; check the passkey and OS pairing: GATT 5").startsWith("The bike did not accept the passkey."))
        assertTrue(userFacingError("BLE response timeout or notification overflow; reconnect before another command").startsWith("The bike stopped responding"))
        assertTrue(userFacingError("Bluetooth disconnected (GATT 8); reconnect").startsWith("The Bluetooth link closed."))
        assertTrue(userFacingError("read failed: cleanup could not be confirmed: Bluetooth connection is closed").contains("confirmed it was released"))
        assertTrue(userFacingError("settings/authentication is unsupported for this motor identity").endsWith("No change was sent."))
    }

    @Test
    fun riderWordingIsStableWhenAppliedTwice() {
        listOf("BLE authentication failed", "BLE response timeout", "Bluetooth disconnected (GATT 8)", "cleanup could not be confirmed: x")
            .forEach { val once = userFacingError(it); assertEquals(once, userFacingError(once)) }
    }

    @Test
    fun unknownErrorsKeepTheirCauseWithoutTheJournalPath() {
        assertEquals("Turn on Bluetooth before connecting", userFacingError("Turn on Bluetooth before connecting"))
        assertEquals("motor refused the value",
            userFacingError("batch stopped at operation 1; 0 completed operations remain applied; inspect /data/user/0/app/files/backups/tx_1.journal: motor refused the value"))
    }

    @Test
    fun saveOutcomeNamesTheSettingThatStopped() {
        val error = "batch stopped at operation 2; 1 completed operations remain applied; inspect /x/tx.journal: BLE response timeout"
        assertEquals("1 of 3 changes were applied before the error. Wheel circumference was not written.",
            saveFailureOutcome(error, listOf("Wired light output", "Wheel circumference", "Lighting reserve")))
        assertEquals("Wired light output may or may not have been written.",
            saveFailureOutcome("batch stopped at operation 1; 0 completed operations remain applied; inspect /x: outcome may be uncertain", listOf("Wired light output")))
        assertEquals("None of the 2 changes were applied. A was not written.",
            saveFailureOutcome("batch stopped at operation 1; 0 completed operations remain applied; inspect /x: no", listOf("A", "B")))
        assertNull(saveFailureOutcome("BLE response timeout", listOf("A")))
    }

    @Test
    fun formatsExtendedEtubeSettingsWithoutActiveConnection() {
        assertEquals("012345", formatSettingValue("bluetooth-passkey", "012345"))
        assertEquals("Bluetooth & ANT+", formatSettingValue("wireless-mode", "3"))
        assertEquals("ANT+ only", formatSettingValue("wireless-mode", "1"))
        assertEquals("17:25", formatSettingValue("display-time", "1045"))
        assertEquals("00:05", formatSettingValue("display-time", "5"))
        assertEquals("Off", formatSettingValue("start-mode", "0"))
        assertEquals("Gear 3", formatSettingValue("start-mode", "3"))
        assertEquals("70 rpm", formatSettingValue("auto-gear-cadence", "70"))
        assertEquals("Assist", formatSettingValue("switch-mode", "2"))
        assertEquals("Shifting", formatSettingValue("switch-mode", "0"))
        assertEquals("Mixed (custom A/B)", formatSettingValue("switch-mode", "1"))
        assertEquals("D-Fly channel 1", formatSettingValue("switch-c-function", "10"))
        assertEquals("Assist mode up", formatSettingValue("switch-a-function", "6"))
    }
}
