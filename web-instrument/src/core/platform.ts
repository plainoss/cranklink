/**
 * platform.ts - Browser platform detection, Screen Wake Lock management,
 * and smart multi-transport negotiation.
 */

export interface PlatformInfo {
  isIOS: boolean;
  isBluefy: boolean;
  isWindows: boolean;
  isMacOS: boolean;
  isAndroid: boolean;
  isLinux: boolean;
  supportsSerial: boolean;
  supportsBluetooth: boolean;
  supportsUsb: boolean;
}

export function detectPlatform(): PlatformInfo {
  if (typeof navigator === "undefined") {
    return {
      isIOS: false,
      isBluefy: false,
      isWindows: false,
      isMacOS: false,
      isAndroid: false,
      isLinux: false,
      supportsSerial: false,
      supportsBluetooth: false,
      supportsUsb: false,
    };
  }

  const ua = navigator.userAgent || "";
  const navPlatform = (navigator as any).userAgentData?.platform || navigator.platform || "";

  // iOS / iPadOS detection
  const isIOS =
    /iPad|iPhone|iPod/.test(ua) ||
    (/Macintosh|MacIntel/.test(navPlatform) && navigator.maxTouchPoints > 1);

  const isBluefy = /Bluefy/i.test(ua);
  const isWindows = /Win/i.test(navPlatform) || /Windows/i.test(ua);
  const isMacOS = /Mac/i.test(navPlatform) && !isIOS;
  const isAndroid = /Android/i.test(ua);
  const isLinux = /Linux/i.test(navPlatform) && !isAndroid;

  const supportsSerial = "serial" in navigator;
  const supportsBluetooth = "bluetooth" in navigator;
  const supportsUsb = "usb" in navigator;

  return {
    isIOS,
    isBluefy,
    isWindows,
    isMacOS,
    isAndroid,
    isLinux,
    supportsSerial,
    supportsBluetooth,
    supportsUsb,
  };
}

export async function checkBluetoothAvailability(): Promise<boolean> {
  if (typeof navigator === "undefined" || !("bluetooth" in navigator)) {
    return false;
  }
  if (typeof (navigator as any).bluetooth?.getAvailability === "function") {
    try {
      return Boolean(await (navigator as any).bluetooth.getAvailability());
    } catch {
      return false;
    }
  }
  return true;
}

let activeWakeLock: any = null;

export async function requestScreenWakeLock(): Promise<boolean> {
  if (typeof navigator === "undefined" || !("wakeLock" in navigator)) {
    return false;
  }
  try {
    if (!activeWakeLock) {
      activeWakeLock = await (navigator as any).wakeLock.request("screen");
      activeWakeLock.addEventListener("release", () => {
        activeWakeLock = null;
      });
      return true;
    }
  } catch {
    // Wake Lock request failed or battery saver active
  }
  return false;
}

export function releaseScreenWakeLock(): void {
  if (activeWakeLock) {
    try {
      activeWakeLock.release().catch(() => {});
    } finally {
      activeWakeLock = null;
    }
  }
}
