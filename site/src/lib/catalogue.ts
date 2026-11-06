import readme from "../../../README.md?raw";
import { readCompatibility } from "./compatibility";

export const { motors, parts } = readCompatibility(readme);
