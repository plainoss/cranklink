/** Concise operator messages; complete errors remain in Diagnostics logs. */
export function userFacingError(error: string): string {
  if (/PC-link stop.*not.*confirmed|stop could not be confirmed|cleanup.*(?:failed|not.*confirmed)/i.test(error)) {
    return "Bluetooth cleanup wasn’t confirmed. Check the bike’s state before reconnecting, then read settings to check the result.";
  }
  if (/BLE authentication failed|BLE passkey must|passkey.*(?:invalid|incorrect)/i.test(error)) {
    return "Bluetooth PIN was not accepted. Reconnect and enter the bike’s six-digit PIN again.";
  }
  if (/drive-unit authentication is not verified/i.test(error)) {
    return "Saving this setting isn’t supported on this motor firmware yet. No change was sent.";
  }
  if (/drive.unit authentication|modern authentication.*(?:reject|fail)|authentication challenge rejected/i.test(error)) {
    return "The motor rejected authorization for this change. Nothing was retried; check Diagnostics for details.";
  }
  if (/BLE response timeout|notification overflow/i.test(error)) {
    return "Bluetooth communication was interrupted. Reconnect and read settings to check the result.";
  }
  if (/GATT Server is disconnected|Bluetooth disconnected|Bluetooth connection is closed/i.test(error)) {
    return "Bluetooth disconnected. Enable the bike’s connection mode and reconnect.";
  }
  if (/browser host operation ble.open failed|Connection attempt failed/i.test(error)) {
    return "Couldn’t connect to the bike. Enable its Bluetooth connection mode and try again.";
  }
  return error.split(/\n\s*at |\nError:/)[0].replace(/^error:\s*/i, "").replace(/\s*\[exit \d+\]\s*$/, "").trim();
}
