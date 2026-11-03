package app.plainoss.cranklink.shared

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class ComponentTypeTest {
    @Test
    fun namesTypeFromModelPrefix() {
        assertEquals("Motor", componentType("DU-EP800-CRG"))
        assertEquals("Rear derailleur", componentType("RD-M8050-GS"))
        assertEquals("Shifter", componentType("SW-E7000-R"))
        assertEquals("Battery", componentType("BT-DN110"))
        assertEquals("Wireless unit", componentType("EW-WU111"))
        assertEquals("Battery mount", componentType("SM-BMR1"))
        assertNull(componentType("XX-1"))
        assertEquals("Motor · DU-EP800", componentLabel("DU-EP800"))
        assertEquals("XX-1", componentLabel("XX-1"))
    }
}
