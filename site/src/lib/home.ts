// Homepage content. Values follow docs/settings.md, docs/bluetooth.md and the README.

export interface Region {
  id: string;
  name: string;
  max: 25 | 32 | 45;
  detail: string;
}
export interface Motor {
  id: string;
  name: string;
  status: string;
  tested: boolean;
  regions: Region[];
}

const europe: Region = {
  id: "eu",
  name: "Europe",
  max: 25,
  detail: "Assist speed adjustable from 15 to 25 km/h.",
};
const unitedStates: Region = {
  id: "us",
  name: "United States",
  max: 32,
  detail: "Assist speed adjustable from 19 to 32 km/h.",
};
const regionOnly = (region: Region): Region => ({
  ...region,
  detail: "Region switch only. This motor has no separate assist speed value.",
});

export const motors: Motor[] = [
  {
    id: "ep801",
    name: "EP801 / EP600 / EP500",
    status:
      "Preview: checked against the protocol, real-bike tests in progress",
    tested: false,
    regions: [
      europe,
      unitedStates,
      {
        id: "c3",
        name: "US Class 3",
        max: 45,
        detail: "Assist up to 45 km/h. Only these motors support Class 3.",
      },
    ],
  },
  {
    id: "ep8",
    name: "EP8",
    status: "Tested on a bike (EP8 CRG), over Bluetooth and wired",
    tested: true,
    regions: [europe, unitedStates],
  },
  {
    id: "e8000",
    name: "E8000 / E8080",
    status: "Preview: reading tested on a bike",
    tested: false,
    regions: [regionOnly(europe), regionOnly(unitedStates)],
  },
  {
    id: "e5100",
    name: "E5100",
    status: "Preview",
    tested: false,
    regions: [europe, unitedStates],
  },
];

export interface Connection {
  id: string;
  name: string;
  columns: { heading: string; items: string[] }[];
}

export const connections: Connection[] = [
  {
    id: "ble",
    name: "Bluetooth",
    columns: [
      {
        heading: "You need",
        items: [
          "A bike with a Bluetooth display or wireless unit, such as SC-EM800, SC-E8000 or EW-WU111",
          "Nothing else to buy",
        ],
      },
      {
        heading: "Works in",
        items: [
          "The web app, in Chrome or Edge on a computer or Android phone",
          "The Android app",
          "The command line tool",
        ],
      },
      {
        heading: "You can change",
        items: [
          "Region and assist speed",
          "Assist profiles, wheel size, display",
          "Di2 parts, in the web app and command line",
        ],
      },
      {
        heading: "Not over Bluetooth",
        items: [
          "Some settings on some bikes, such as light output",
          "Recovering a motor stuck in its bootloader",
          "Firmware flashing is experimental and not yet run on a bike",
        ],
      },
    ],
  },
  {
    id: "usb",
    name: "Wired (SM-PCE02)",
    columns: [
      {
        heading: "You need",
        items: [
          "A Shimano SM-PCE02 USB adapter",
          "A USB cable to your computer or Android phone",
        ],
      },
      {
        heading: "Works in",
        items: [
          "The web app over WebUSB, in Chrome or Edge",
          "The command line tool on Linux, macOS and Windows",
        ],
      },
      {
        heading: "You can change",
        items: [
          "Everything Bluetooth can",
          "Settings that need a direct connection to the bike",
          "Firmware, and recovery from bootloader (command line)",
        ],
      },
      {
        heading: "Good to know",
        items: [
          "Needed on some bikes, such as an EP8 with an SC-E7000 display",
          "Bootloader recovery is tested on an EP8 CRG",
        ],
      },
    ],
  },
];

export interface SettingRow {
  name: string;
  what: string;
  values: string;
}
export interface SettingGroup {
  id: string;
  name: string;
  summary: string;
  intro: string;
  note: string;
  rows: SettingRow[];
}

export const settingGroups: SettingGroup[] = [
  {
    id: "assist",
    name: "Assist feel",
    summary: "Torque, character, start",
    intro:
      "EP-series motors keep two assist profiles, each with its own Eco, Trail and Boost. Switch between them on the bike.",
    note: "Bluetooth or wired. E8000 and E7000 use Boost and Trail levels and presets instead.",
    rows: [
      {
        name: "Peak torque",
        what: "Limits motor torque in each mode",
        values: "Nm, within the motor's factory limits",
      },
      {
        name: "Assist character",
        what: "How strongly assist reacts to your pedalling",
        values: "Levels, per mode",
      },
      {
        name: "Start response",
        what: "How quickly assist builds when you start pedalling",
        values: "Levels, per mode",
      },
      {
        name: "Maximum power",
        what: "Power ceiling per mode, where the motor supports it",
        values: "W, within the region's limits",
      },
      {
        name: "Active profile and names",
        what: "Which profile is in use, and what the display calls it",
        values: "Profile 1 or 2, up to 10 characters",
      },
    ],
  },
  {
    id: "bike",
    name: "Bike and wheel",
    summary: "Wheel size, speedometer, walk",
    intro: "Keep speed and distance right after a tyre change.",
    note: "Bluetooth or wired.",
    rows: [
      {
        name: "Wheel circumference",
        what: "Rollout of your tyre, used for speed and distance",
        values: "mm",
      },
      {
        name: "Speedometer calibration",
        what: "Fine correction of the speed shown",
        values: "−5 to 5 %, in 0.5 % steps",
      },
      {
        name: "Walk assist",
        what: "Push assist while walking the bike",
        values: "On or off",
      },
    ],
  },
  {
    id: "lights",
    name: "Lights",
    summary: "Lamp, power, timer",
    intro: "For lights powered from the e-bike battery.",
    note: "Light output needs a wired connection on some bikes.",
    rows: [
      {
        name: "Lamp",
        what: "Whether a light is connected to the motor",
        values: "Off or on",
      },
      {
        name: "Light power",
        what: "Power sent to the lights",
        values: "1.35 to 27 W",
      },
      {
        name: "Lighting time",
        what: "How long the light stays on",
        values: "Minutes",
      },
    ],
  },
  {
    id: "shifting",
    name: "Shifting",
    summary: "Auto gear, start gear",
    intro: "For bikes with Di2 shifting or an automatic hub run by the motor.",
    note: "Bluetooth or wired.",
    rows: [
      {
        name: "Auto shift cadence",
        what: "The cadence automatic shifting aims for",
        values: "rpm",
      },
      {
        name: "Start gear",
        what: "Gear to shift to when you stop",
        values: "A gear, or off",
      },
      {
        name: "Shift advice",
        what: "Shows on the display when to shift",
        values: "On or off",
      },
      {
        name: "Gearing",
        what: "Chainring and cassette teeth, and number of gears",
        values: "Teeth and gears",
      },
    ],
  },
  {
    id: "service",
    name: "Maintenance",
    summary: "Service reminders",
    intro: "Set or clear the service reminder on the display.",
    note: "Bluetooth or wired.",
    rows: [
      {
        name: "Service alert",
        what: "Show a reminder on the display",
        values: "On or off",
      },
      {
        name: "Service distance and date",
        what: "When the reminder appears",
        values: "km, or a date",
      },
    ],
  },
  {
    id: "di2",
    name: "Di2 and buttons",
    summary: "Levers, derailleurs, passkey",
    intro:
      "Road, gravel and e-bike Di2 parts. In the web app and command line, not the Android app.",
    note: "Bluetooth or wired. Hardware testing for Di2 parts is in progress.",
    rows: [
      {
        name: "Button functions",
        what: "What each lever or switch button does, including swapping assist and shift on e-bike switches",
        values: "Shift up or down, assist up or down, display",
      },
      {
        name: "Multi-shift speed",
        what: "How fast the rear derailleur shifts several gears at once",
        values: "Off, very slow to super fast",
      },
      {
        name: "Synchronized shifting",
        what: "Pause before the front derailleur follows",
        values: "Normal, slow, very slow",
      },
      {
        name: "Bluetooth passkey and radio mode",
        what: "On wireless units and 12-speed wireless derailleurs",
        values: "6-digit passkey",
      },
      {
        name: "Display and clock",
        what: "E-bike display settings and time",
        values: "Per display",
      },
    ],
  },
];

export interface CliExample {
  id: string;
  name: string;
  note: string;
  code: string;
}

export const cliExamples: CliExample[] = [
  {
    id: "settings",
    name: "Read and change settings",
    note: "Every write saves a backup first, checks the serial of the motor you meant, and reads the value back.",
    code: `$ cranklink ble scan
$ cranklink --port 'ble:DEVICE_ID' probe
$ cranklink --port 'ble:DEVICE_ID' -s SERIAL \\
    settings read --output settings.json
$ cranklink --port 'ble:DEVICE_ID' -s SERIAL \\
    settings set destination 1 --backup before.json
$ cranklink --port 'ble:DEVICE_ID' -s SERIAL \\
    settings restore before.json`,
  },
  {
    id: "ble-flash",
    name: "Flash over Bluetooth",
    note: "Experimental and not yet run on hardware. Updates a working EP8's D and M firmware together. Bring your own firmware files; none are bundled.",
    code: `$ cranklink firmware inspect DUEP800-D.4.2.0.dat \\
    --companion DUEP800-M.4.1.11.dat

# A dry run by default: checks everything, writes nothing
$ cranklink --port 'ble:DEVICE_ID' -s SERIAL \\
    firmware flash ble-pair ...

# Add --write only when the dry run passes`,
  },
  {
    id: "recover",
    name: "Recover a bricked motor (USB)",
    note: "For a motor stuck in its bootloader. Needs an SM-PCE02, stable power and your own .dat files. Tested on an EP8 CRG.",
    code: `$ cranklink --port usb firmware probe \\
    --target dcas --recovery

$ cranklink --port usb -s SERIAL \\
    firmware flash dcas DUEP800-D.4.2.0.dat \\
    --companion DUEP800-M.4.1.11.dat \\
    --expect-sha256 D_SHA256 --recovery

# Then the same with --journal D.journal.jsonl --write`,
  },
  {
    id: "script",
    name: "Scripting and JSON",
    note: "Batch plans run several writes on one connection, each with its own backup.",
    code: `$ export CRANKLINK_PORT=usb
$ cranklink --json -s SERIAL settings read > now.json
$ cranklink -s SERIAL settings set \\
    profile1.boost.torque 85 --dry-run
$ cranklink -s SERIAL settings batch plan.json \\
    --backup-prefix run-01`,
  },
];
