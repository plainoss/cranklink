package work.plainoss.cranklink.shared

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

class SessionControllerTest {
    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun cancellingQueuedDisconnectStillWaitsForTheSaveAndReleasesTheSession() = runTest {
        val backend = FakeBackend()
        val controller = SessionController(backend)
        controller.connect()
        controller.stage("lamp", "1")
        val saving = CompletableDeferred<Unit>()
        val finishSave = CompletableDeferred<Unit>()
        val cleanedUp = CompletableDeferred<Unit>()
        backend.handler = { request ->
            when (request.string("op")) {
                "commit" -> { saving.complete(Unit); finishSave.await(); null }
                "disconnect" -> { delay(1); cleanedUp.complete(Unit); null }
                else -> null
            }
        }
        val save = launch { controller.commit() }
        saving.await()
        val disconnect = launch { controller.disconnect() }
        runCurrent()
        disconnect.cancel()
        runCurrent()
        assertFalse(cleanedUp.isCompleted)
        assertEquals(AppPhase.Saving, controller.state.value.phase)
        finishSave.complete(Unit)
        save.join()
        disconnect.join()
        assertTrue(cleanedUp.isCompleted)
        assertEquals("1", backend.snapshot["snapshot"]!!.jsonObject["values"]!!.jsonObject["lamp"]!!.jsonObject.string("value"))
        assertEquals(AppPhase.Disconnected, controller.state.value.phase)
        assertEquals(1, backend.requests.count { it.string("op") == "commit" })
        assertEquals(1, backend.requests.count { it.string("op") == "disconnect" })
    }

    @Test
    fun disconnectAfterAFailedConnectKeepsTheFailureReason() = runTest {
        val backend = FakeBackend()
        val controller = SessionController(backend)
        backend.handler = { request ->
            if (request.string("op") == "connect") error("this bike reports a Di2 shifting system") else null
        }
        controller.connect()
        controller.disconnect()
        assertEquals(AppPhase.Disconnected, controller.state.value.phase)
        assertEquals("this bike reports a Di2 shifting system", controller.state.value.error)
        assertEquals(1, backend.requests.count { it.string("op") == "disconnect" })
    }

    @Test
    fun cancelledOperationsCompleteCleanupBeforeReturning() = runTest {
        for (operation in listOf("connect", "read", "commit", "restore")) {
            val backend = FakeBackend().apply { backups = listOf("test.backup.json") }
            val controller = SessionController(backend)
            if (operation != "connect") controller.connect()
            if (operation == "commit") controller.stage("lamp", "1")
            val entered = CompletableDeferred<Unit>()
            val cleanedUp = CompletableDeferred<Unit>()
            backend.handler = { request ->
                when (request.string("op")) {
                    operation -> {
                        entered.complete(Unit)
                        awaitCancellation()
                    }
                    "disconnect" -> {
                        // Cleanup must finish even though its caller was cancelled.
                        delay(1)
                        cleanedUp.complete(Unit)
                        null
                    }
                    else -> null
                }
            }
            val job = launch {
                when (operation) {
                    "connect" -> controller.connect()
                    "read" -> controller.refresh()
                    "commit" -> controller.commit()
                    "restore" -> controller.restore("test.backup.json")
                }
            }
            entered.await()
            job.cancelAndJoin()
            assertTrue(job.isCancelled, operation)
            assertTrue(cleanedUp.isCompleted, "$operation left the native session open")
            assertEquals(1, backend.requests.count { it.string("op") == "disconnect" }, operation)
            assertEquals(AppPhase.Disconnected, controller.state.value.phase, operation)
            assertTrue(controller.state.value.staged.isEmpty(), operation)
            assertEquals(null, controller.state.value.identity, operation)
        }
    }

    @Test
    fun newE600xIdentitiesSelectTheirFirmwareBoundSchema() {
        for ((series, name, version, slug) in listOf(
            MotorCase(0x0a, "DU-E6001", "3.0.0.0", "e6001"),
            MotorCase(0x0c, "DU-E6050", "3.9.0.0", "e6001"),
            MotorCase(0x15, "DU-E6002", "3.2.99.0", "e6002-pre33"),
            MotorCase(0x15, "DU-E6012", "3.3.0.0", "e6002"),
            MotorCase(0x15, "DU-E6002", "3.10.0.0", "e6002"),
            MotorCase(0x15, "DU-E6002", "4.0.0.0", "e6002"),
        )) {
            val identity = e600xFixture(series, slug, version)
                .withIdentity("model_name", JsonPrimitive(name))["identity"]!!.jsonObject
            assertEquals(slug, TuningRules.parseIdentity(identity).modelSlug)
            assertEquals(name, TuningRules.parseIdentity(identity).model)
            assertFailsWith<IllegalArgumentException> {
                TuningRules.parseIdentity(JsonObject(identity + ("model_unit" to JsonPrimitive(1))))
            }
        }
        for ((version, slug) in listOf("3.2.0.0" to "e6002", "3.3.0.0" to "e6002-pre33")) {
            assertFailsWith<IllegalArgumentException> {
                TuningRules.parseIdentity(e600xFixture(0x15, slug, version)["identity"]!!.jsonObject)
            }
        }
        for (version in listOf("", "3.3", "+3.3.0.0", "3.3.0.x", "65536.0.0.0")) {
            assertFailsWith<IllegalArgumentException> {
                TuningRules.parseIdentity(e600xFixture(0x15, "e6002", version)["identity"]!!.jsonObject)
            }
        }
    }

    @Test
    fun e600xHardwareBoundsAndRegionOnlySavesStayWithinTheReportedSchema() = runTest {
        for ((series, slug, version) in listOf(
            Triple(0x0a, "e6001", "3.0.0.0"), Triple(0x0c, "e6001", "3.9.0.0"),
            Triple(0x15, "e6002-pre33", "3.2.0.0"), Triple(0x15, "e6002", "3.3.0.0"),
        )) {
            val backend = FakeBackend(e600xFixture(series, slug, version))
            val controller = SessionController(backend)
            controller.connect()
            assertEquals(AppPhase.Ready, controller.state.value.phase)
            assertTrue(backend.requests.single().bool("master"))
            val settings = controller.state.value.settings
            assertEquals(if (slug == "e6002") 4 else 3, settings.size)
            assertEquals(2000.0, settings.single { it.name == "tire-circumference" }.min)
            assertEquals(2400.0, settings.single { it.name == "tire-circumference" }.max)
            assertEquals(45.0, settings.single { it.name == "mounting-angle" }.max)
            assertEquals(listOf("0", "1"), settings.single { it.name == "lamp" }.choices)
            assertFalse(settings.any { it.name == "speed-limit" || it.name.startsWith("profile") })
            if (slug == "e6002") {
                controller.stage("destination", "3")
                assertTrue(controller.state.value.staged.isEmpty())
                controller.stage("destination", "4")
                assertEquals(mapOf("destination" to "4"), controller.state.value.staged)
                controller.commit()
                assertEquals("4", controller.state.value.values["destination"])
                assertEquals(setOf("destination"), backend.requests.single { it.string("op") == "commit" }["changes"]!!.jsonObject.keys)
            } else {
                assertFailsWith<IllegalArgumentException> {
                    TuningRules.parseSchema(JsonArray(listOf(schema("destination", 0, 4))), slug)
                }
            }
            assertEquals(null, TuningRules.speedRange(slug, 1))
        }
    }

    @Test
    fun marketPresentationUsesTheBackupOrLiveMotorModelWithoutInventingCutoffs() {
        assertEquals(listOf("Europe (EU)", "United States (US)", "Korea"),
            listOf("0", "1", "4").map { formatSettingValue("destination", it, "e6002") })
        assertFalse(settingPresentation("destination", "e6002").description.contains("speed", ignoreCase = true))
        assertEquals("4 (15–25 km/h)", formatSettingValue("destination", "4", "ep800-crg"))
        assertEquals("5 (19–45 km/h)", formatSettingValue("destination", "5", "ep801"))
        assertEquals("Region 4", formatSettingValue("destination", "4"))
    }

    @Test
    fun modernTorqueUsesOnlyVerifiedFactoryTablesAndModeSubsets() {
        val modern = TuningRules.parseIdentity(fixture().withIdentity("model_series", JsonPrimitive(0x35))
            .withIdentity("settings_model", JsonPrimitive("ep801"))["identity"]!!.jsonObject)
        for ((maximum, eco, boost) in listOf(Triple("85", "70", "34"), Triple("50", "43", "27"))) {
            val bounds = mapOf("torque-minimum" to "20", "torque-maximum" to maximum)
            TuningRules.validateCombination(modern, bounds, mapOf("profile1.eco.torque" to eco, "profile1.boost.torque" to boost))
            assertFailsWith<IllegalArgumentException> {
                TuningRules.validateCombination(modern, bounds, mapOf("profile1.eco.torque" to maximum))
            }
            assertFailsWith<IllegalArgumentException> {
                TuningRules.validateCombination(modern, bounds, mapOf("profile1.boost.torque" to "20"))
            }
        }
        for ((minimum, maximum, value) in listOf(Triple("25", "80", "49"), Triple("20", "80", "27"), Triple("20", "100", "56"))) {
            assertFailsWith<IllegalStateException> {
                TuningRules.validateCombination(modern, mapOf("torque-minimum" to minimum, "torque-maximum" to maximum),
                    mapOf("profile1.trail.torque" to value))
            }
        }
        val legacy = TuningRules.parseIdentity(fixture()["identity"]!!.jsonObject)
        TuningRules.validateCombination(legacy, mapOf("torque-minimum" to "25", "torque-maximum" to "80"),
            mapOf("profile1.trail.torque" to "49"))
    }

    @Test
    fun liveModernConstraintsProvideDiscreteChoicesWithoutExposingFactorySettings() = runTest {
        val raw = JsonArray(listOf(schema("profile1.eco.torque", 20, 85), schema("profile1.boost.torque", 20, 85),
            schema("profile1.trail.character", 0, 14)))
        val bounds = buildJsonObject { put("torque_minimum", 20); put("torque_maximum", 50) }
        val settings = TuningRules.parseSchema(raw, "ep801", bounds)
        assertEquals(listOf("20", "24", "27", "30", "33", "36", "40", "43"), settings[0].choices)
        assertEquals(listOf("27", "30", "33", "36", "40", "43", "46", "50"), settings[1].choices)
        assertTrue(settings.all { it.writable })
        assertFailsWith<IllegalArgumentException> { TuningRules.validate(settings[0], "50") }
        for (constraints in listOf(null, buildJsonObject { put("torque_minimum", 25); put("torque_maximum", 80) })) {
            val unsupported = TuningRules.parseSchema(raw, "ep801", constraints)
            assertFalse(unsupported[0].writable)
            assertFalse(unsupported[1].writable)
            assertTrue(unsupported[2].writable, "unknown torque limits must not disable other controls")
        }
        val base = fixture(extraSchema = listOf(schema("profile1.eco.torque", 20, 85)),
            extraValues = mapOf("profile1.eco.torque" to "20"))
            .withIdentity("model_series", JsonPrimitive(0x35)).withIdentity("settings_model", JsonPrimitive("ep801"))
        val controller = SessionController(FakeBackend(JsonObject(base + ("constraints" to bounds))))
        controller.connect()
        assertEquals(AppPhase.Ready, controller.state.value.phase)
        assertFalse(controller.state.value.settings.any { it.name.startsWith("torque-") })
        controller.stage("profile1.eco.torque", "50")
        assertTrue(controller.state.value.staged.isEmpty())
        controller.stage("profile1.eco.torque", "43")
        assertEquals(mapOf("profile1.eco.torque" to "43"), controller.state.value.staged)
    }

    @Test
    fun connectsOnlyToVerifiedMotorAndScalesWireSchema() = runTest {
        val controller = SessionController(FakeBackend())
        controller.connect()
        val state = controller.state.value
        assertEquals(AppPhase.Ready, state.phase)
        assertEquals("010203040506", state.identity?.serial)
        val speed = state.settings.single { it.name == "speed-limit" }
        assertEquals(15.0, speed.min)
        assertEquals(32.0, speed.max)
        assertEquals(1.0, speed.step)
        assertEquals("25", state.values["speed-limit"])
    }

    @Test
    fun componentIdentityAndUnknownSchemaAreRejected() = runTest {
        val component = fixture().withIdentity("model_unit", JsonPrimitive(1))
        val componentController = SessionController(FakeBackend(component))
        componentController.connect()
        assertEquals(AppPhase.Disconnected, componentController.state.value.phase)
        assertNotNull(componentController.state.value.error)

        val unknownSchema = fixture().withSchema(schema("derailleur-adjustment", -10, 10))
        val unknownController = SessionController(FakeBackend(unknownSchema))
        unknownController.connect()
        assertEquals(AppPhase.Disconnected, unknownController.state.value.phase)
        assertTrue(unknownController.state.value.values.isEmpty())
    }

    @Test
    fun incompleteSnapshotAndMismatchedSchemaCannotBecomeWritable() = runTest {
        val incomplete = fixture().withSnapshotValues(emptyMap())
        val controller = SessionController(FakeBackend(incomplete))
        controller.connect()
        assertEquals(AppPhase.Disconnected, controller.state.value.phase)
        controller.stage("lamp", "1")
        assertTrue(controller.state.value.staged.isEmpty())

        val mismatched = SessionController(FakeBackend(fixture().withIdentity("settings_model", JsonPrimitive("ep801"))))
        mismatched.connect()
        assertEquals(AppPhase.Disconnected, mismatched.state.value.phase)
    }

    @Test
    fun stageRejectsOutOfScopeUnavailableReadonlyAndInvalidValues() = runTest {
        val backend = FakeBackend(fixture(extraSchema = listOf(
            schema("mounting-angle", 0, 60, readOnly = true),
            schema("lighting-time", 0, 180),
        ), extraValues = mapOf("mounting-angle" to "0"), unavailable = mapOf("lighting-time" to "unsupported")))
        val controller = SessionController(backend)
        controller.connect()
        for ((name, value) in listOf(
            "shifting-system" to "1", "firmware" to "1", "mounting-angle" to "2",
            "lighting-time" to "30", "lamp" to "7", "speed-limit" to "25.5", "speed-limit" to "NaN",
        )) {
            controller.stage(name, value)
            assertTrue(controller.state.value.staged.isEmpty(), "$name=$value must not be staged")
            assertNotNull(controller.state.value.error)
        }
        assertFalse(backend.requests.any { it.string("op") == "commit" })
    }

    @Test
    fun stagesOnlyChangesAndDiscardNeverWrites() = runTest {
        val backend = FakeBackend()
        val controller = SessionController(backend)
        controller.connect()
        controller.stage("lamp", "1.0")
        assertEquals(mapOf("lamp" to "1"), controller.state.value.staged)
        controller.stage("lamp", "0.0")
        assertTrue(controller.state.value.staged.isEmpty())
        controller.stage("lamp", "1")
        controller.discard()
        assertTrue(controller.state.value.staged.isEmpty())
        assertEquals(listOf("connect"), backend.requests.map { it.string("op") })
    }

    @Test
    fun speedUsesSelectedRegionAndCommitOrdersDestinationFirst() = runTest {
        val backend = FakeBackend()
        val controller = SessionController(backend)
        controller.connect()
        controller.stage("speed-limit", "32")
        assertTrue(controller.state.value.staged.isEmpty())
        controller.stage("lamp", "1")
        controller.stage("destination", "1")
        controller.stage("speed-limit", "32")
        controller.commit()
        val request = backend.requests.single { it.string("op") == "commit" }
        assertEquals("010203040506", request.string("serial"))
        assertEquals("destination", (request["changes"] as JsonObject).keys.first())
        assertEquals("32", controller.state.value.values["speed-limit"])
        assertTrue(controller.state.value.staged.isEmpty())
        assertEquals(AppPhase.Ready, controller.state.value.phase)
    }

    @Test
    fun commitFailureInvalidatesStaleStateAndFindsNewBackups() = runTest {
        val backend = FakeBackend()
        val controller = SessionController(backend)
        controller.connect()
        controller.stage("lamp", "1")
        backend.handler = { request ->
            if (request.string("op") == "commit") {
                backend.backups = listOf("tx.001.backup.json")
                error("batch stopped; one earlier operation remains applied")
            }
            null
        }
        controller.commit()
        val failed = controller.state.value
        assertEquals(AppPhase.Disconnected, failed.phase)
        assertEquals(null, failed.identity)
        assertTrue(failed.values.isEmpty())
        assertTrue(failed.staged.isEmpty())
        assertEquals(listOf("tx.001.backup.json"), failed.backups)
        assertTrue(failed.status.contains("may remain applied"))
        controller.commit()
        assertEquals(1, backend.requests.count { it.string("op") == "commit" })
    }

    @Test
    fun incompleteReadbackCannotBeReportedAsSuccessfulSave() = runTest {
        val backend = FakeBackend()
        val controller = SessionController(backend)
        controller.connect()
        controller.stage("lamp", "1")
        backend.handler = { if (it.string("op") == "commit") fixture() else null }
        controller.commit()
        assertEquals(AppPhase.Disconnected, controller.state.value.phase)
        assertTrue(controller.state.value.error.orEmpty().contains("readbacks"))
    }

    @Test
    fun failedRefreshInvalidatesValuesAndChangedSerialCannotReplaceTarget() = runTest {
        val backend = FakeBackend()
        val controller = SessionController(backend)
        controller.connect()
        backend.handler = { if (it.string("op") == "read") fixture().withIdentity("serial", JsonPrimitive("AABBCCDDEEFF")) else null }
        controller.refresh()
        assertEquals(AppPhase.Disconnected, controller.state.value.phase)
        assertTrue(controller.state.value.values.isEmpty())
        assertTrue(controller.state.value.error.orEmpty().contains("different drive unit"))
    }

    @Test
    fun restoreRequiresKnownBasenameIdleStateAndMatchingSerial() = runTest {
        val backend = FakeBackend().apply { backups = listOf("original.backup.json") }
        val controller = SessionController(backend)
        controller.connect()
        controller.restore("../original.backup.json")
        controller.restore("missing.backup.json")
        controller.stage("lamp", "1")
        controller.restore("original.backup.json")
        assertFalse(backend.requests.any { it.string("op") == "restore" })
        controller.discard()
        controller.restore("original.backup.json")
        val request = backend.requests.single { it.string("op") == "restore" }
        assertEquals("010203040506", request.string("serial"))
        assertEquals("original.backup.json", request.string("file"))
        assertEquals(AppPhase.Ready, controller.state.value.phase)
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun backendOperationsAreSerializedAndStagingBlockedDuringSave() = runTest {
        val entered = CompletableDeferred<Unit>()
        val release = CompletableDeferred<Unit>()
        val backend = FakeBackend()
        val controller = SessionController(backend)
        controller.connect()
        controller.stage("lamp", "1")
        backend.handler = {
            if (it.string("op") == "commit") {
                entered.complete(Unit)
                release.await()
            }
            null
        }
        val saving = launch { controller.commit() }
        entered.await()
        val reading = launch { controller.refresh() }
        runCurrent()
        assertFalse(backend.requests.any { it.string("op") == "read" })
        controller.stage("lamp", "0")
        assertEquals("1", controller.state.value.staged["lamp"])
        release.complete(Unit)
        saving.join()
        reading.join()
        assertEquals(listOf("connect", "commit", "read"), backend.requests.map { it.string("op") })
    }

    @Test
    fun olderMotorFamilyCannotReceiveNewerSchemaFields() = runTest {
        val legacy = fixture().withIdentity("model_series", JsonPrimitive(8))
            .withIdentity("settings_model", JsonPrimitive("e6000"))
        val controller = SessionController(FakeBackend(legacy))
        controller.connect()
        assertEquals(AppPhase.Disconnected, controller.state.value.phase)
        assertTrue(controller.state.value.error.orEmpty().contains("motor family"))
    }

    @Test
    fun localBackupsRefreshWithoutConnectionAndPreservePendingChanges() = runTest {
        val backend = FakeBackend().apply { backups = listOf("imported.backup.json") }
        val controller = SessionController(backend)
        controller.refreshBackups()
        assertEquals(AppPhase.Disconnected, controller.state.value.phase)
        assertEquals(listOf("imported.backup.json"), controller.state.value.backups)
        controller.connect()
        controller.stage("lamp", "1")
        backend.backups = listOf("imported.backup.json", "new.backup.json")
        controller.refreshBackups()
        assertEquals(AppPhase.Ready, controller.state.value.phase)
        assertEquals(mapOf("lamp" to "1"), controller.state.value.staged)
        assertEquals(2, controller.state.value.backups.size)
        assertFalse(backend.requests.any { it.string("op") in listOf("commit", "restore", "disconnect") })
    }

    @Test
    fun calendarAliasesCannotBeStagedTogetherIncludingUnset() = runTest {
        val dateSchema = JsonObject(schema("maintenance-date", 0, 0) + mapOf(
            "value_type" to JsonPrimitive("date"), "nullable" to JsonPrimitive(true),
        ))
        val backend = FakeBackend(fixture(
            extraSchema = listOf(dateSchema, schema("maintenance-year", 0, 99),
                schema("maintenance-month", 1, 12), schema("maintenance-day", 1, 31)),
            extraValues = mapOf("maintenance-date" to "2024-01-01", "maintenance-year" to "24",
                "maintenance-month" to "1", "maintenance-day" to "1"),
        ))
        val controller = SessionController(backend)
        controller.connect()
        for (date in listOf("2025-03-01", "unset")) {
            controller.discard()
            controller.stage("maintenance-date", date)
            for ((field, value) in mapOf("maintenance-year" to "25", "maintenance-month" to "3", "maintenance-day" to "2")) {
                controller.stage(field, value)
                assertEquals(mapOf("maintenance-date" to date), controller.state.value.staged)
                assertTrue(controller.state.value.error.orEmpty().contains("individual fields"))
            }
        }
        controller.discard()
        controller.stage("maintenance-year", "25")
        controller.stage("maintenance-date", "2025-03-01")
        assertEquals(mapOf("maintenance-year" to "25"), controller.state.value.staged)
        assertNotNull(controller.state.value.error)
        assertEquals(listOf("connect"), backend.requests.map { it.string("op") })
    }

    @Test
    fun profileNamesDatesAndModernSpeedAreValidated() {
        val name = TuningSetting("profile1.name", "Profile name", "profiles", 0.0, 0.0, 1.0,
            emptyList(), "", true, valueType = "string")
        assertEquals("Cargo", TuningRules.validate(name, "Cargo"))
        assertTrue(runCatching { TuningRules.validate(name, "Too long profile") }.isFailure)
        assertTrue(runCatching { TuningRules.validate(name, "Ride\n") }.isFailure)
        val date = name.copy(name = "maintenance-date", valueType = "date", nullable = true)
        assertEquals("2024-02-29", TuningRules.validate(date, "2024-02-29"))
        assertEquals("unset", TuningRules.validate(date, "unset"))
        assertTrue(runCatching { TuningRules.validate(date, "2025-02-29") }.isFailure)
        assertEquals(19.0..45.0, TuningRules.speedRange("ep801", 5))
        assertEquals(null, TuningRules.speedRange("ep800-crg", 5))
    }

    @Test
    fun physicalUnitPresentationPreservesCanonicalValuesAndBounds() {
        val light = TuningRules.parseSchema(JsonArray(listOf(schema("light-output", 135, 2700))), "ep800-crg").single()
        assertEquals(135.0, light.min)
        assertEquals(2700.0, light.max)
        assertEquals(1.0, light.step)
        assertEquals("W", light.unit)
        assertEquals(1.35, light.displayMin)
        assertEquals(27.0, light.displayMax)
        assertEquals(0.01, light.displayStep)
        assertEquals(13.75, light.displayNumber(1375.0))
        assertEquals("13.75", light.inputText("1375"))
        assertEquals("1375", light.backendInput("13.75"))
        assertEquals("139", light.backendInput(light.inputText("139")))
        assertEquals("13.75 W", formatSettingValue("light-output", "1375"))
        assertEquals("1.39 W", formatSettingValue("light-output", "139"))
        val low = assertFailsWith<IllegalArgumentException> { light.backendInput("0") }
        assertTrue(low.message.orEmpty().contains("1.35–27 W"))
        val precision = assertFailsWith<IllegalArgumentException> { light.backendInput("13.751") }
        assertTrue(precision.message.orEmpty().contains("0.01 W"))
    }

    @Test
    fun calibrationAndCalendarUsePhysicalInputsWithoutChangingWireUnits() {
        val calibration = TuningRules.parseSchema(JsonArray(listOf(schema("display-speed-adjustment", -50, 50, step = 5))), "ep800-crg").single()
        assertEquals(-50.0, calibration.min)
        assertEquals(5.0, calibration.step)
        assertEquals(-5.0, calibration.displayMin)
        assertEquals(5.0, calibration.displayMax)
        assertEquals(0.5, calibration.displayStep)
        assertEquals("%", calibration.unit)
        assertEquals(-1.5, calibration.displayNumber(-15.0))
        assertEquals("-1.5", calibration.inputText("-15"))
        assertEquals("-15", calibration.backendInput("-1.5"))
        assertEquals("-1.5%", formatSettingValue("display-speed-adjustment", "-15"))
        val precision = assertFailsWith<IllegalArgumentException> { calibration.backendInput("-1.55") }
        assertTrue(precision.message.orEmpty().contains("0.5%"))
        val year = TuningRules.parseSchema(JsonArray(listOf(schema("maintenance-year", 0, 99))), "ep800-crg").single()
        assertEquals("2025", year.inputText("25"))
        assertEquals("25", year.backendInput("2025"))
        assertEquals("2025", formatSettingValue("maintenance-year", "25"))
    }

    @Test
    fun formattedNamesDatesAndChoicesPreserveTheirMeaning() {
        val name = TuningSetting("profile1.name", "Profile name", "profiles", 0.0, 0.0, 1.0,
            emptyList(), "", true, valueType = "string")
        for (text in listOf("Cargo.0", "001.0", "")) {
            assertEquals(text, name.inputText(text))
            assertEquals(text, name.backendInput(text))
            assertEquals(text, formatSettingValue("profile1.name", text))
        }
        val date = name.copy(name = "maintenance-date", valueType = "date", nullable = true)
        assertEquals("2025-03-01", date.inputText("2025-03-01"))
        assertEquals("2025-03-01", date.backendInput("2025-03-01"))
        assertEquals("unset", date.inputText("unset"))
        assertEquals("unset", date.backendInput("unset"))
        assertEquals("Not set", formatSettingValue("maintenance-date", "unset"))
        assertEquals("On", formatSettingValue("lamp", "1"))
        assertEquals("Disabled", formatSettingValue("walk-assist", "0"))
        assertEquals("Sport", formatSettingValue("assist-pattern", "6"))
        assertEquals("Eco / Trail / Boost", formatSettingValue("profile1.display-oem", "1"))
        assertEquals("Level 3", formatSettingValue("profile1.eco.character", "3"))
        assertEquals("45°", formatSettingValue("mounting-angle", "45"))
    }

    @Test
    fun everyKnownTuningFieldHasAnIntentionalDescription() {
        val known = mutableListOf("lamp", "light-output", "lighting-time", "walk-assist", "tire-circumference",
            "mounting-angle", "destination", "assist-pattern", "speed-limit", "display-speed-adjustment",
            "maintenance-alert", "maintenance-distance", "maintenance-year", "maintenance-month",
            "maintenance-day", "maintenance-date", "torque-minimum", "torque-maximum")
        for (profile in 1..2) {
            known += "profile$profile.name"
            for (factory in listOf("", "-oem")) {
                known += "profile$profile.display$factory"
                for (mode in listOf("eco", "trail", "boost")) for (field in listOf("torque", "character", "start")) {
                    known += "profile$profile.$mode.$field$factory"
                }
            }
        }
        for (mode in listOf("eco", "trail", "boost")) known += "max-power.$mode"
        known.forEach { name ->
            assertTrue(TuningRules.allowedName(name), name)
            val metadata = settingPresentation(name)
            assertTrue(metadata.description.isNotBlank(), name)
            assertFalse(metadata.description.startsWith("Value reported"), name)
            assertFalse(metadata.unit in listOf("enum", "profile", "raw index", "0.01 W", "0.1 percent"), name)
        }
        assertTrue(settingPresentation("profile1.eco.torque").description.contains("torque limit"))
        assertTrue(settingPresentation("profile1.eco.character").description.contains("pedaling effort"))
        assertTrue(settingPresentation("profile1.eco.start-oem").description.contains("startup response"))
        assertTrue(settingPresentation("profile1.eco.start-oem").description.startsWith("Factory default"))
    }

    @Test
    fun physicalEditsKeepCanonicalStateAndNativeSaveContract() = runTest {
        val backend = FakeBackend(fixture(
            extraSchema = listOf(schema("light-output", 135, 2700), schema("display-speed-adjustment", -50, 50, step = 5)),
            extraValues = mapOf("light-output" to "1350", "display-speed-adjustment" to "15"),
        ))
        val controller = SessionController(backend)
        controller.connect()
        val light = controller.state.value.settings.single { it.name == "light-output" }
        val calibration = controller.state.value.settings.single { it.name == "display-speed-adjustment" }
        assertEquals("13.5", light.inputText(controller.state.value.values.getValue(light.name)))
        assertFailsWith<IllegalArgumentException> { light.backendInput("13.755") }
        assertTrue(controller.state.value.staged.isEmpty())
        assertFalse(backend.requests.any { it.string("op") == "commit" })
        controller.stage(light.name, light.backendInput("13.75"))
        controller.stage(calibration.name, calibration.backendInput("-1.5"))
        assertEquals("1350", controller.state.value.values[light.name])
        assertEquals("1375", controller.state.value.staged[light.name])
        assertEquals("-15", controller.state.value.staged[calibration.name])
        controller.commit()
        val changes = backend.requests.single { it.string("op") == "commit" }["changes"] as JsonObject
        assertEquals("1375", changes.string(light.name))
        assertEquals("-15", changes.string(calibration.name))
        assertEquals("1375", controller.state.value.values[light.name])
        assertEquals("-15", controller.state.value.values[calibration.name])
        assertTrue(controller.state.value.staged.isEmpty())
    }
}

private data class MotorCase(val series: Int, val name: String, val version: String, val slug: String)

private fun e600xFixture(series: Int, slug: String, version: String): JsonObject {
    val specs = listOf(schema("lamp", 0, 1, choices = listOf(0, 1)),
        schema("tire-circumference", 2000, 2400), schema("mounting-angle", 0, 45)) +
        if (slug == "e6002") listOf(schema("destination", 0, 4, choices = listOf(0, 1, 4))) else emptyList()
    val values = mapOf("lamp" to "0", "tire-circumference" to "2280", "mounting-angle" to "0") +
        if (slug == "e6002") mapOf("destination" to "0") else emptyMap()
    val base = fixture().withIdentity("model_series", JsonPrimitive(series))
        .withIdentity("settings_model", JsonPrimitive(slug)).withIdentity("dcas_version", JsonPrimitive(version))
        .withSnapshotValues(values.mapValues { buildJsonObject { put("value", it.value) } })
    return JsonObject(base + ("schema" to JsonArray(specs)))
}

private class FakeBackend(var snapshot: JsonObject = fixture()) : AppBackend {
    val requests = mutableListOf<JsonObject>()
    var backups = emptyList<String>()
    var handler: (suspend (JsonObject) -> JsonObject?)? = null

    override suspend fun execute(request: JsonObject): JsonObject {
        requests += request
        handler?.invoke(request)?.let { return it }
        when (request.string("op")) {
            "commit" -> {
                val old = snapshot["snapshot"]!!.jsonObject["values"]!!.jsonObject.toMutableMap()
                (request["changes"] as JsonObject).forEach { (name, value) ->
                    old[name] = buildJsonObject { put("value", value); put("raw", "") }
                }
                val originalSnapshot = snapshot["snapshot"]!!.jsonObject
                snapshot = JsonObject(snapshot + ("snapshot" to JsonObject(originalSnapshot + ("values" to JsonObject(old)))))
            }
            "backups", "disconnect" -> return buildJsonObject { put("backups", JsonArray(backups.map(::JsonPrimitive))) }
        }
        return JsonObject(snapshot + ("backups" to JsonArray(backups.map(::JsonPrimitive))))
    }
}

private fun fixture(
    extraSchema: List<JsonObject> = emptyList(),
    extraValues: Map<String, String> = emptyMap(),
    unavailable: Map<String, String> = emptyMap(),
): JsonObject = buildJsonObject {
    put("identity", Json.parseToJsonElement("""{
      "model_name":"DU-EP800","serial":"010203040506","slot":0,"model_series":35,"model_unit":0,
      "settings_model":"ep800-crg","dcas_version":"4.2.0.0","rx_version":"4.2.0.0"
    }"""))
    put("schema", JsonArray(listOf(
        schema("lamp", 0, 2, choices = listOf(0, 1, 2)),
        schema("destination", 0, 4, choices = listOf(0, 1, 3, 4)),
        schema("speed-limit", 1500, 3200, step = 100, scale = 100),
    ) + extraSchema))
    put("snapshot", buildJsonObject {
        put("model", "DU-EP800/DU-EP800-CRG")
        put("slot", 0)
        put("values", buildJsonObject {
            (mapOf("lamp" to "0", "destination" to "3", "speed-limit" to "25") + extraValues).forEach { (name, value) ->
                put(name, buildJsonObject { put("value", value); put("raw", "") })
            }
        })
        put("unavailable", buildJsonObject { unavailable.forEach { (name, value) -> put(name, value) } })
    })
    put("backups", JsonArray(emptyList()))
}

private fun schema(name: String, min: Int, max: Int, step: Int = 1, scale: Int = 1,
                   choices: List<Int> = emptyList(), readOnly: Boolean = false): JsonObject = buildJsonObject {
    put("name", name)
    put("min_raw", min)
    put("max_raw", max)
    put("step_raw", step)
    put("wire_units_per_input", scale)
    put("choices", JsonArray(choices.map(::JsonPrimitive)))
    put("unit", "")
    put("read_only", readOnly)
    put("value_type", "number")
}

private fun JsonObject.withIdentity(name: String, value: JsonPrimitive): JsonObject =
    JsonObject(this + ("identity" to JsonObject(this["identity"]!!.jsonObject + (name to value))))

private fun JsonObject.withSchema(spec: JsonObject): JsonObject = JsonObject(this + ("schema" to JsonArray(listOf(spec))))

private fun JsonObject.withSnapshotValues(values: Map<String, JsonObject>): JsonObject = JsonObject(this +
    ("snapshot" to JsonObject(this["snapshot"]!!.jsonObject + ("values" to JsonObject(values)))))
