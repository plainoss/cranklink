package work.plainoss.cranklink.shared

/**
 * Rider-facing wording for native and Bluetooth failures, matching the web app's
 * web-instrument/src/domain/errors.ts. Unrecognised errors keep their cause, minus
 * journal paths and repeated context.
 */
fun userFacingError(error: String): String {
    val text = error.trim()
    return when {
        CLEANUP_UNCONFIRMED.containsMatchIn(text) ->
            "The Bluetooth link closed before the bike confirmed it was released. Turn on the bike’s connection mode, then reconnect to read its current settings."
        PASSKEY_REJECTED.containsMatchIn(text) ->
            "The bike did not accept the passkey. Reconnect and enter its six-digit E-TUBE passkey again."
        IDENTITY_UNSUPPORTED.containsMatchIn(text) ->
            "Settings aren’t supported for this motor identity yet. No change was sent."
        FIRMWARE_UNSUPPORTED.containsMatchIn(text) ->
            "Saving this setting isn’t supported on this motor firmware yet. No change was sent."
        SERIAL_MISMATCH.containsMatchIn(text) ->
            "The connected motor’s serial doesn’t match the one this session expects. No change was sent."
        AUTH_REJECTED.containsMatchIn(text) ->
            "The motor rejected authorization for this change. Nothing was retried."
        TIMEOUT.containsMatchIn(text) ->
            "The bike stopped responding over Bluetooth. Reconnect to read its current settings."
        LINK_CLOSED.containsMatchIn(text) ->
            "The Bluetooth link closed. Turn on the bike’s connection mode and reconnect."
        else -> rawError(text)
    }
}

/**
 * States what a stopped save left on the bike, as the web app's failure banner does.
 * [labels] are the staged settings in the order they were sent.
 */
fun saveFailureOutcome(error: String, labels: List<String>): String? {
    val batch = BATCH_STOPPED.find(error) ?: return null
    val failedIndex = batch.groupValues[1].toInt() - 1
    val applied = batch.groupValues[2].toInt()
    val total = maxOf(labels.size, failedIndex + 1)
    val failedLabel = labels.getOrNull(failedIndex) ?: "Change ${failedIndex + 1}"
    val failed = if (UNCERTAIN.containsMatchIn(error)) "$failedLabel may or may not have been written."
        else "$failedLabel was not written."
    val appliedText = when {
        total == 1 -> ""
        applied == 0 -> "None of the $total changes were applied. "
        else -> "$applied of $total changes were applied before the error. "
    }
    return appliedText + failed
}

/** Drop journal paths, nested context and trailing reconnect hints from a native error chain. */
private fun rawError(error: String): String {
    val withoutBatch = BATCH_STOPPED.find(error)?.let { error.substring(it.range.last + 1).trimStart(':', ' ') } ?: error
    return withoutBatch.replace(Regex("^error:\\s*", RegexOption.IGNORE_CASE), "").trim().ifEmpty { error }
}

private val CLEANUP_UNCONFIRMED = Regex("PC-link stop.*not.*confirmed|stop could not be confirmed|cleanup.*(?:failed|not.*confirmed)", RegexOption.IGNORE_CASE)
private val PASSKEY_REJECTED = Regex("BLE authentication failed|BLE passkey must|passkey.*(?:invalid|incorrect)", RegexOption.IGNORE_CASE)
private val IDENTITY_UNSUPPORTED = Regex("unsupported for this motor identity", RegexOption.IGNORE_CASE)
private val FIRMWARE_UNSUPPORTED = Regex("protected writes require an unambiguous", RegexOption.IGNORE_CASE)
private val SERIAL_MISMATCH = Regex("serial mismatch", RegexOption.IGNORE_CASE)
private val AUTH_REJECTED = Regex("authentication (?:challenge|reply) rejected", RegexOption.IGNORE_CASE)
private val TIMEOUT = Regex("BLE response timeout|notification overflow|notification size or queue limit|Bluetooth \\w+ timed out|Bluetooth connection timed out", RegexOption.IGNORE_CASE)
private val LINK_CLOSED = Regex("Bluetooth disconnected|Bluetooth connection is closed|Bluetooth is not connected|BLE session failed|GATT Server is disconnected", RegexOption.IGNORE_CASE)
/** The core's batch planner (src/settings/batch.rs) reports how far a save got this way. */
private val BATCH_STOPPED = Regex("batch stopped at operation (\\d+); (\\d+) completed operations? remain applied(?:; inspect [^:]*)?", RegexOption.IGNORE_CASE)
private val UNCERTAIN = Regex("outcome may be uncertain", RegexOption.IGNORE_CASE)
