/**
 * navigation.ts - Single source for destinations, their routes, and titles.
 */

import type { LucideIcon } from "lucide-react";
import { Compass, Cog, Disc3, Gauge, Layers, ShieldCheck, Sliders, Terminal, Wrench, Archive } from "lucide-react";
import type { TuneCategory } from "./components/views/TuneView.tsx";

export type Destination =
  | { goal: "overview" }
  | { goal: "tune"; category: TuneCategory }
  | { goal: "firmware" }
  | { goal: "backup" }
  | { goal: "diagnostics" };

export interface NavItem {
  destination: Destination;
  label: string;
  icon: LucideIcon;
  testId: string;
}

export const TUNE_ITEMS: NavItem[] = [
  { destination: { goal: "tune", category: "geometry" }, label: "Bike & Geometry", icon: Compass, testId: "nav-group-geometry" },
  { destination: { goal: "tune", category: "profiles" }, label: "Assist Profiles", icon: Sliders, testId: "nav-group-profiles" },
  { destination: { goal: "tune", category: "drivetrain" }, label: "Drivetrain & Di2", icon: Cog, testId: "nav-group-drivetrain" },
  { destination: { goal: "tune", category: "maintenance" }, label: "Service & Maintenance", icon: Wrench, testId: "nav-group-maintenance" },
  { destination: { goal: "tune", category: "oem" }, label: "OEM Factory Bounds", icon: ShieldCheck, testId: "nav-group-oem" },
  { destination: { goal: "tune", category: "all" }, label: "All Settings", icon: Layers, testId: "nav-group-all" },
];

export const OVERVIEW_ITEM: NavItem = { destination: { goal: "overview" }, label: "Overview", icon: Gauge, testId: "nav-section-overview" };

export const TOOL_ITEMS: NavItem[] = [
  { destination: { goal: "firmware" }, label: "Firmware & Recovery", icon: Disc3, testId: "nav-section-firmware" },
  { destination: { goal: "backup" }, label: "Backup & Restore", icon: Archive, testId: "nav-section-journal" },
  { destination: { goal: "diagnostics" }, label: "Diagnostics & Logs", icon: Terminal, testId: "nav-section-diagnostics" },
];

const ALL_ITEMS = [OVERVIEW_ITEM, ...TUNE_ITEMS, ...TOOL_ITEMS];
const CATEGORIES = TUNE_ITEMS.map((item) => (item.destination as { category: TuneCategory }).category);

export function destinationFromPath(pathname: string): Destination {
  const [, first, second] = pathname.split("/");
  if (first === "overview" || first === "firmware" || first === "backup" || first === "diagnostics") return { goal: first };
  const category = CATEGORIES.includes(second as TuneCategory) ? (second as TuneCategory) : "geometry";
  return { goal: "tune", category };
}

export function pathFor(destination: Destination): string {
  return destination.goal === "tune" ? `/tune/${destination.category}` : `/${destination.goal}`;
}

export function sameDestination(a: Destination, b: Destination): boolean {
  return pathFor(a) === pathFor(b);
}

export function titleFor(destination: Destination): string {
  return ALL_ITEMS.find((item) => sameDestination(item.destination, destination))?.label ?? "cranklink";
}
