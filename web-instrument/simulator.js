// In-browser GATT Simulator for Shimano STEPS E-TUBE Drive Units
// Supports DU-EP800-CRG (legacy 0x23), DU-EP600 (modern 0x30), and DU-EP801 (modern 0x35)
// Full register fidelity for Rust WASM `probe`, `settings read`, `settings set`, and `settings batch`.
import { ETUBE_SERVICE, characteristicUuid } from "./ble.js";

export class Characteristic extends EventTarget {
  constructor(id, gateway) {
    super();
    this.id = id;
    this.gateway = gateway;
    this.uuid = characteristicUuid(id);
    this.properties = {
      read: true,
      write: true,
      writeWithoutResponse: false,
      notify: true,
      indicate: false,
    };
    this.value = null;
  }

  async readValue() {
    return new DataView(Uint8Array.from([1, 2, 3, 4, 5, 6, 7, 8]).buffer);
  }

  async startNotifications() {
    return this;
  }

  async writeValueWithResponse(bytes) {
    this.gateway.write(this.id, [...bytes]);
  }

  async writeValueWithoutResponse(bytes) {
    this.gateway.write(this.id, [...bytes]);
  }

  emit(bytes) {
    this.value = new DataView(Uint8Array.from(bytes).buffer);
    this.dispatchEvent(new Event("characteristicvaluechanged"));
  }
}

export class DeviceSimulator extends EventTarget {
  constructor({
    model = "ep800-crg",
    authFailure = null,
    paddedProductSerial = false,
    fault = "none",
    disconnectOnWrite = false,
    networkLatencyMs = 0,
    initialPreset = null,
    logWirePackets = true,
  } = {}) {
    super();

    if (!["ep800-crg", "ep600", "ep801"].includes(model)) {
      throw new Error("unknown simulated motor model");
    }

    this.id = `simulated-etube-${model}`;
    this.name = {
      "ep800-crg": "Simulated DU-EP800-CRG",
      ep600: "Simulated DU-EP600",
      ep801: "Simulated DU-EP801",
    }[model];

    this.model = model;
    this.authFailure = authFailure || (fault === "auth_reject" ? "status" : null);
    this.paddedProductSerial = paddedProductSerial || model === "ep801";
    this.fault = fault;
    this.disconnectOnWrite = disconnectOnWrite || fault === "disconnect_on_write";
    this.networkLatencyMs = networkLatencyMs;
    this.logWirePackets = logWirePackets;

    this.authStart = [];
    this.authProof = [];
    this.unitAuthenticated = false;
    this.connections = 0;
    this.writes = [];
    this.wireLog = [];

    // Initialize register defaults
    this._defaultRegisters = this._createDefaultRegisters(initialPreset);
    this.registers = JSON.parse(JSON.stringify(this._defaultRegisters));

    // GATT Characteristics: 0x2af3..0x2aff
    this.chars = [0x2af3, 0x2af4, 0x2af9, 0x2afa, 0x2afd, 0x2afe, 0x2aff].map(
      (id) => new Characteristic(id, this),
    );

    this.gatt = {
      connected: false,
      connect: async () => {
        if (this.fault === "disconnect_on_connect") {
          this.triggerDisconnect();
          throw new Error("Simulated connection failure (disconnect_on_connect)");
        }
        this.connections++;
        this.gatt.connected = true;
        return {
          getPrimaryService: async (id) => {
            if (id !== ETUBE_SERVICE) throw new Error("wrong simulated service");
            return { getCharacteristics: async () => this.chars };
          },
        };
      },
      disconnect: () => {
        this.triggerDisconnect();
      },
    };
  }

  _createDefaultRegisters(preset) {
    const regs = {
      speedLimitCentiKmh: 2500, // 0x16 0xb4 read / 0x16 0xb0 write (default 2500 = 25.0 km/h)
      destinationRegion: 1,     // 0x16 0xac read / 0x16 0xa8 write (default 1 = Europe)
      tireCircumferenceMm: 2200,// 0x16 0x9c or 0x35 0x04 read / 0x16 0x98 or 0x35 0x00 write
      lampState: 0,             // 0x10 0x14 read / 0x10 0x10 write
      lightOutputWattCenti: 200,// 0x10 0x1c read / 0x10 0x18 write
      lightingTimeMinutes: 30,  // 0x16 0xa4 read / 0x16 0xa0 write
      walkAssistMode: 2,        // 0x16 0x6c read / 0x16 0x68 write (2 = Enabled)
      mountingAngle: 12,        // 0x16 0x5c read / 0x16 0x58 write
      assistPattern: 5,         // 0x16 0x84 read / 0x16 0x80 write
      displaySpeedAdjustment: 0,// 0x16 0xd4 read / 0x16 0xd0 write
      torqueMinimum: 20,
      torqueMaximum: 85,
      shiftingSystem: 0,        // 0x0b 0x84
      rearMaxGear: 11,          // 0x0b 0x84
      frontTeeth: 38,           // 0x0a 0x4c
      rearTeeth: 11,            // 0x0b 0x4c
      rearTeethPattern: 1,      // 0x0b 0xb4
      autoGearCadence: 650,     // 0x0b 0x94 (65.0 rpm)
      autoGearMode: 0,          // 0x0b 0x04
      shiftingAdvice: 0,        // 0x0b 0xdc
      startMode: 0,             // 0x0b 0x8c
      shiftLimitUp: 255,        // 0x0b 0x64
      shiftLimitDown: 255,      // 0x0b 0x64
      maintenanceAlert: 0,      // 0x48 0x44
      maintenanceDistance: 2500,// 0x48 0x4c
      maintenanceCalendar: [26, 12, 1, 0], // 2026-12-01 (0x48 0x54)
      maxPower: {
        1: 250,
        2: 400,
        3: 500,
      },
      profiles: {
        "0:1": { torque: 34, character: 2, start: 2 },
        "0:2": { torque: 56, character: 5, start: 3 },
        "0:3": { torque: 85, character: 9, start: 4 },
        "1:1": { torque: 27, character: 1, start: 1 },
        "1:2": { torque: 49, character: 4, start: 2 },
        "1:3": { torque: 85, character: 7, start: 3 },
        "2:1": { torque: 34, character: 2, start: 2 },
        "2:2": { torque: 56, character: 5, start: 3 },
        "2:3": { torque: 85, character: 9, start: 3 },
        "3:1": { torque: 27, character: 1, start: 1 },
        "3:2": { torque: 49, character: 4, start: 2 },
        "3:3": { torque: 85, character: 7, start: 3 },
      },
      profileDisplay: {
        0: 1,
        1: 1,
        2: 1,
        3: 1,
      },
      profileNames: {
        1: "DYNAMIC   ",
        2: "EXPLORER  ",
      },
    };

    if (preset === "eu_default" || preset === "eu") {
      regs.speedLimitCentiKmh = 2500;
      regs.destinationRegion = 1;
      regs.tireCircumferenceMm = 2280;
      regs.lampState = 1;
      regs.walkAssistMode = 2;
    } else if (preset === "us_commuter" || preset === "us") {
      regs.speedLimitCentiKmh = 3200;
      regs.destinationRegion = 1;
      regs.tireCircumferenceMm = 2200;
      regs.lampState = 1;
      regs.walkAssistMode = 2;
    } else if (preset === "cargo_heavy" || preset === "cargo") {
      regs.speedLimitCentiKmh = 2500;
      regs.destinationRegion = 1;
      regs.tireCircumferenceMm = 2150;
      regs.lampState = 1;
      regs.walkAssistMode = 2;
      regs.torqueMaximum = 85;
    }

    return regs;
  }

  get lamp() {
    return this.registers.lampState;
  }

  set lamp(val) {
    this.registers.lampState = val;
  }

  setFault(fault) {
    this.fault = fault;
    if (fault === "auth_reject") {
      this.authFailure = "status";
    } else if (fault === "disconnect_on_write") {
      this.disconnectOnWrite = true;
    } else if (fault === "none") {
      this.authFailure = null;
      this.disconnectOnWrite = false;
    }
  }

  triggerDisconnect() {
    this.gatt.connected = false;
    this.dispatchEvent(new Event("gattserverdisconnected"));
  }

  getRegisters() {
    return JSON.parse(JSON.stringify(this.registers));
  }

  resetRegisters() {
    this.registers = JSON.parse(JSON.stringify(this._defaultRegisters));
  }

  getWireLog() {
    return [...this.wireLog];
  }

  emit(id, data) {
    if (this.logWirePackets) {
      this.wireLog.push({
        timestamp: Date.now(),
        characteristic: id,
        direction: "out",
        bytes: [...data],
      });
    }
    const char = this.chars.find((c) => c.id === id);
    if (!char) throw new Error(`Unknown characteristic id 0x${id.toString(16)}`);

    if (this.networkLatencyMs > 0) {
      setTimeout(() => char.emit(data), this.networkLatencyMs);
    } else {
      char.emit(data);
    }
  }

  write(id, b) {
    if (this.logWirePackets) {
      this.wireLog.push({
        timestamp: Date.now(),
        characteristic: id,
        direction: "in",
        bytes: [...b],
      });
    }

    const unitAuth =
      id === 0x2afe &&
      ((b[1] === 0x16 && [0xd8, 0xe0].includes(b[2])) ||
        (b[1] === 0x33 && [0x68, 0x70].includes(b[2])));
    this.writes.push({ id, bytes: id === 0x2af3 || unitAuth ? "redacted" : b });

    if (id === 0x2afa) {
      const response = {
        3: [0x23, 0],
        4: [0x24, 0x83],
        5: [0x25, 4, 4, 0],
        6: [0x26, 0],
        12: [0x2c, 0],
        13: [0x2d, 1, 0xff, 0],
        27: [0x3b, 8, 32, 0, 0],
        29: [0x3d, 0, 0],
      }[b[1]];
      if (!response) throw new Error(`unexpected simulated PCE command ${b[1]}`);
      this.emit(0x2af9, response);
      return;
    }

    if (id === 0x2afe) {
      // Local gateway queries on slot 0
      if (b[0] === 0 && b[1] === 2 && b[2] === 0x14 && b[3] === 13) {
        const series = { "ep800-crg": 0x23, ep600: 0x30, ep801: 0x35 }[this.model];
        this.emit(0x2afd, [0, 2, 0x16, 13, series, 0]);
        return;
      }
      if (b[0] === 0 && b[1] === 1 && b[2] === 0x14) {
        this.emit(0x2afd, [0, 1, 0x16, 0, 0, 8, 32, 0, 0, 0xff]);
        return;
      }
      if (b[0] === 0 && b[1] === 1 && b[2] === 0x1c) {
        const series = { "ep800-crg": 0x23, ep600: 0x30, ep801: 0x35 }[this.model];
        this.emit(0x2afd, [0, 1, 0x1e, series, 0]);
        return;
      }

      if (b[0] !== 13) throw new Error("unexpected simulated unit slot");
      const [slot, group, command] = b;
      const params = b.slice(3);

      if (group === 0x32 && command === 0x30) return;

      if (group === 0x33 && [0x68, 0x70].includes(command)) {
        this.authFragment(slot, command, params);
        return;
      }

      const isWrite =
        (group === 0x0a && command === 0x48) ||
        (group === 0x0b && [0x00, 0x48, 0x60, 0x80, 0x88, 0x90, 0xb0, 0xd8].includes(command)) ||
        (group === 0x10 && [0x10, 0x18].includes(command)) ||
        (group === 0x16 &&
          [0x58, 0x68, 0x80, 0x98, 0xa0, 0xa8, 0xb0, 0xd0, 0xf8].includes(command)) ||
        (group === 0x17 && [0x10, 0x18, 0x20, 0x28].includes(command)) ||
        (group === 0x35 && command === 0x00) ||
        (group === 0x48 && [0x40, 0x48, 0x50].includes(command));

      if (isWrite) {
        if (this.disconnectOnWrite || this.fault === "disconnect_on_write") {
          this.triggerDisconnect();
          return;
        }
        if (this.fault === "write_timeout") {
          return;
        }
        if (this.fault === "readonly_rejection") {
          this.emit(0x2afd, [slot, group, command + 3]);
          return;
        }
      }

      let response = [slot, group, command + 2];

      if (group === 0x32 && command === 0x10) {
        response.push(params[0] !== undefined ? params[0] : b[3], 0, 0xff, 0xff);
      } else if (group === 0x16 && command === 0x7c) {
        response.push(
          ...new TextEncoder().encode(
            this.model === "ep800-crg" ? "00-CRG\0" : "00-STD\0",
          ),
        );
      } else if (group === 1 && command === 0x2c) {
        // DCAS firmware version: EP800-CRG -> 4.2.0.0 (0x42, 0, 0), EP600/EP801 -> 4.6.0.1 (0x46, 0, 1)
        response.push(...(this.model === "ep800-crg" ? [0x42, 0, 0] : [0x46, 0, 1]));
      } else if (group === 1 && command === 0x84) {
        // RX firmware version: 4.1.11.0 (0x41, 11, 0)
        response.push(0x41, 11, 0);
      } else if (group === 1 && command === 0x34) {
        // Model identity: [0, series, 0]
        const series = { "ep800-crg": 0x23, ep600: 0x30, ep801: 0x35 }[this.model];
        response.push(0, series, 0);
      } else if (group === 1 && command === 0x3c) {
        response.push(1, 2, 3, 4, 5, 6);
      } else if (group === 1 && command === 0xa4) {
        response.push(0, ...new TextEncoder().encode("12345"));
        if (this.paddedProductSerial) response.push(255, 255);
      } else if (group === 1 && command === 0xac) {
        response.push(...new TextEncoder().encode("ABCDEF"));
        if (this.paddedProductSerial) response.push(255, 255);
      } else if (group === 0x10 && command === 0x14) {
        response.push(this.registers.lampState);
      } else if (group === 0x10 && command === 0x10) {
        this.registers.lampState = params[0] !== undefined ? params[0] : b[3];
      } else if (group === 0x10 && command === 0x1c) {
        const lo = this.registers.lightOutputWattCenti;
        response.push(lo & 0xff, (lo >> 8) & 0xff, 0xff, 0xff);
      } else if (group === 0x10 && command === 0x18) {
        this.registers.lightOutputWattCenti = params[0] | (params[1] << 8);
      } else if (group === 0x16 && command === 0xb4) {
        const spd = this.registers.speedLimitCentiKmh;
        response.push(spd & 0xff, (spd >> 8) & 0xff, 0xff, 0xff);
      } else if (group === 0x16 && command === 0xb0) {
        this.registers.speedLimitCentiKmh = params[0] | (params[1] << 8);
      } else if (group === 0x16 && command === 0xac) {
        response.push(params[0] ?? 1, this.registers.destinationRegion, 0xff, 0xff);
      } else if (group === 0x16 && command === 0xa8) {
        this.registers.destinationRegion =
          params.length > 1 ? params[1] : params[0];
      } else if (group === 0x16 && command === 0x9c) {
        const tc = this.registers.tireCircumferenceMm;
        response.push(tc & 0xff, (tc >> 8) & 0xff, 0, 0);
      } else if (group === 0x16 && command === 0x98) {
        this.registers.tireCircumferenceMm = params[0] | (params[1] << 8);
      } else if (group === 0x35 && command === 0x04) {
        const tc = this.registers.tireCircumferenceMm;
        response.push(tc & 0xff, (tc >> 8) & 0xff, 0, 0);
      } else if (group === 0x35 && command === 0x00) {
        this.registers.tireCircumferenceMm = params[0] | (params[1] << 8);
      } else if (group === 0x16 && command === 0x6c) {
        response.push(this.registers.walkAssistMode);
      } else if (group === 0x16 && command === 0x68) {
        this.registers.walkAssistMode = params[0];
      } else if (group === 0x16 && command === 0x5c) {
        const ma = this.registers.mountingAngle;
        response.push(ma & 0xff, (ma >> 8) & 0xff, 0, 0);
      } else if (group === 0x16 && command === 0x58) {
        this.registers.mountingAngle = params[0] | (params[1] << 8);
      } else if (group === 0x16 && command === 0xa4) {
        const lt = this.registers.lightingTimeMinutes;
        response.push(lt & 0xff, (lt >> 8) & 0xff, 0xff, 0xff);
      } else if (group === 0x16 && command === 0xa0) {
        this.registers.lightingTimeMinutes = params[0] | (params[1] << 8);
      } else if (group === 0x16 && command === 0x84) {
        response.push(this.registers.assistPattern || 5, 0, 0, 0);
      } else if (group === 0x16 && command === 0x80) {
        this.registers.assistPattern = params[0];
      } else if (group === 0x16 && command === 0xd4) {
        response.push((this.registers.displaySpeedAdjustment || 0) & 0xff);
      } else if (group === 0x16 && command === 0xd0) {
        const raw = params[0];
        this.registers.displaySpeedAdjustment = raw > 127 ? raw - 256 : raw;
      } else if (group === 0x16 && command === 0xfc) {
        const p = params[0] !== undefined ? params[0] : 2;
        const m = params[1] !== undefined ? params[1] : 1;
        const key = `${p}:${m}`;
        const prof = this.registers.profiles?.[key] || {
          torque: 85,
          character: 5,
          start: 3,
        };
        response.push(p, m, prof.torque, prof.character, prof.start, 0x21, 0);
      } else if (group === 0x16 && command === 0xf8) {
        const p = params[0];
        const m = params[1];
        const key = `${p}:${m}`;
        if (!this.registers.profiles) this.registers.profiles = {};
        if (!this.registers.profiles[key]) this.registers.profiles[key] = {};
        if (params[2] !== undefined) this.registers.profiles[key].torque = params[2];
        if (params[3] !== undefined) this.registers.profiles[key].character = params[3];
        if (params[4] !== undefined) this.registers.profiles[key].start = params[4];
      } else if (group === 0x17 && command === 0x24) {
        const selector = params[0] || 0x50;
        const isChunk0 = (selector & 1) === 0;
        const profNum = (selector & 0x10) === 0 ? 2 : 1;
        const nameStr =
          this.registers.profileNames?.[profNum] ||
          (profNum === 1 ? "DYNAMIC   " : "EXPLORER  ");
        const rawBytes = new TextEncoder().encode(nameStr.padEnd(10, " ").slice(0, 10));
        const chunk = isChunk0 ? rawBytes.slice(0, 5) : rawBytes.slice(5, 10);
        response.push(selector, ...chunk);
      } else if (group === 0x17 && command === 0x20) {
        const selector = params[0] || 0x50;
        const isChunk0 = (selector & 1) === 0;
        const profNum = (selector & 0x10) === 0 ? 2 : 1;
        const current = (this.registers.profileNames?.[profNum] || "          ").padEnd(10, " ");
        const chunkStr = new TextDecoder().decode(Uint8Array.from(params.slice(1, 6))).replace(/\0/g, " ");
        this.registers.profileNames[profNum] = isChunk0
          ? chunkStr.padEnd(5, " ") + current.slice(5, 10)
          : current.slice(0, 5) + chunkStr.padEnd(5, " ");
      } else if (group === 0x17 && command === 0x1c) {
        response.push(
          this.registers.torqueMinimum || 20,
          this.registers.torqueMaximum || 85,
          0,
          0,
        );
      } else if (group === 0x17 && command === 0x18) {
        this.registers.torqueMinimum = params[0];
        this.registers.torqueMaximum = params[1];
      } else if (group === 0x17 && command === 0x2c) {
        const p = params[0] ?? 0;
        response.push(p, this.registers.profileDisplay?.[p] ?? 1, 0, 0);
      } else if (group === 0x17 && command === 0x28) {
        const p = params[0] ?? 0;
        if (!this.registers.profileDisplay) this.registers.profileDisplay = {};
        this.registers.profileDisplay[p] = params[1] ?? 1;
      } else if (group === 0x17 && command === 0x14) {
        const m = params[0] ?? 1;
        const w = this.registers.maxPower?.[m] ?? 500;
        response.push(m, w & 0xff, (w >> 8) & 0xff, 0);
      } else if (group === 0x17 && command === 0x10) {
        const m = params[0] ?? 1;
        if (!this.registers.maxPower) this.registers.maxPower = {};
        this.registers.maxPower[m] = params[1] | (params[2] << 8);
      } else if (group === 0x0b && command === 0xdc) {
        response.push(this.registers.shiftingAdvice ?? 0);
      } else if (group === 0x0b && command === 0xd8) {
        this.registers.shiftingAdvice = params[0] ?? 0;
      } else if (group === 0x0b && command === 0x04) {
        response.push(this.registers.autoGearMode ?? 0);
      } else if (group === 0x0b && command === 0x00) {
        this.registers.autoGearMode = params[0] ?? 0;
      } else if (group === 0x0b && command === 0xb4) {
        const rtp = this.registers.rearTeethPattern ?? 1;
        response.push(rtp & 0xff, (rtp >> 8) & 0xff, 255, 255);
      } else if (group === 0x0b && command === 0xb0) {
        this.registers.rearTeethPattern = params[0] | (params[1] << 8);
      } else if (group === 0x0b && command === 0x94) {
        const cad = this.registers.autoGearCadence ?? 650;
        response.push(cad & 0xff, (cad >> 8) & 0xff, 0, 0, 0, 0, 0);
      } else if (group === 0x0b && command === 0x90) {
        this.registers.autoGearCadence = params[0] | (params[1] << 8);
      } else if (group === 0x0b && command === 0x84) {
        response.push(
          this.registers.shiftingSystem ?? 0,
          this.registers.rearMaxGear ?? 11,
          0,
          0,
        );
      } else if (group === 0x0b && command === 0x80) {
        this.registers.shiftingSystem = params[0];
        this.registers.rearMaxGear = params[1];
      } else if (group === 0x0a && command === 0x4c) {
        response.push(1, this.registers.frontTeeth ?? 38, 0, 0);
      } else if (group === 0x0a && command === 0x48) {
        this.registers.frontTeeth = params[1];
      } else if (group === 0x0b && command === 0x4c) {
        response.push(0, this.registers.rearTeeth ?? 11, 0);
      } else if (group === 0x0b && command === 0x48) {
        this.registers.rearTeeth = params[1];
      } else if (group === 0x0b && command === 0x8c) {
        response.push(this.registers.startMode ?? 0, 0, 0, 0);
      } else if (group === 0x0b && command === 0x88) {
        this.registers.startMode = params[0];
      } else if (group === 0x0b && command === 0x64) {
        response.push(
          this.registers.shiftLimitUp ?? 255,
          this.registers.shiftLimitDown ?? 255,
          0,
          0,
        );
      } else if (group === 0x0b && command === 0x60) {
        this.registers.shiftLimitUp = params[0];
        this.registers.shiftLimitDown = params[1];
      } else if (group === 0x48 && command === 0x44) {
        response.push(this.registers.maintenanceAlert ?? 0);
      } else if (group === 0x48 && command === 0x40) {
        this.registers.maintenanceAlert = params[0];
      } else if (group === 0x48 && command === 0x4c) {
        const md = this.registers.maintenanceDistance ?? 2500;
        response.push(md & 0xff, (md >> 8) & 0xff, (md >> 16) & 0xff, (md >> 24) & 0xff);
      } else if (group === 0x48 && command === 0x48) {
        this.registers.maintenanceDistance =
          params[0] | (params[1] << 8) | (params[2] << 16) | (params[3] << 24);
      } else if (group === 0x48 && command === 0x54) {
        const cal = this.registers.maintenanceCalendar || [26, 12, 1, 0];
        response.push(...cal);
      } else if (group === 0x48 && command === 0x50) {
        this.registers.maintenanceCalendar = [
          params[0] ?? 26,
          params[1] ?? 12,
          params[2] ?? 1,
          params[3] ?? 0,
        ];
      } else {
        response = [slot, group, command + 3];
      }

      this.emit(0x2afd, response);
    }
  }

  authFragment(slot, command, parameters) {
    if (this.model === "ep800-crg") throw new Error("modern auth on legacy fixture");
    const frames = command === 0x68 ? this.authStart : this.authProof;
    const index = frames.length;
    if (parameters.length !== 7 || parameters[0] !== [0x16, 0x26, 0x34][index])
      throw new Error("invalid/out-of-order simulated auth fragment");
    if (index === 2 && (parameters[5] !== 255 || parameters[6] !== 255))
      throw new Error("invalid simulated auth padding");
    frames.push(parameters.slice(1, index === 2 ? 5 : 7));
    if (index !== 2) return;

    const actual = frames.flat().map((v) => v.toString(16).padStart(2, "0")).join("");
    const expected =
      command === 0x68
        ? "718dd37093b36dc5c414917637a03804"
        : "ab536f5f7be07fc9a91387c76dba82b6";
    frames.length = 0;
    if (actual !== expected) throw new Error("simulated authentication proof mismatch");

    if (command === 0x68) {
      const cipher = "1a80b6dec2c77407612bb9f4ced9a088"
        .match(/../g)
        .map((v) => parseInt(v, 16));
      for (const [selector, offset, length] of [
        [0x16, 0, 6],
        [0x26, 6, 6],
        [0x34, 12, 4],
      ]) {
        const data = cipher.slice(offset, offset + length);
        if (length === 4) data.push(255, 255);
        this.emit(0x2afd, [slot, 0x33, 0x6a, selector, ...data]);
      }
    } else {
      const isFailed = !!this.authFailure || this.fault === "auth_reject";
      this.unitAuthenticated = !isFailed;
      this.emit(
        0x2afd,
        this.unitAuthenticated
          ? [slot, 0x33, 0x72, 0xff, 0xff, 0xff, 0xff]
          : [slot, 0x33, 0x73, 0x45, 0, 0xff, 0],
      );
    }
  }
}

export const Gateway = DeviceSimulator;

export function installMockBluetooth(options) {
  const gateway = new DeviceSimulator(options);
  Object.defineProperty(navigator, "bluetooth", {
    configurable: true,
    value: { requestDevice: async () => gateway },
  });
  window.estepsBleMock = gateway;
  return { simulated: true, id: gateway.id };
}
