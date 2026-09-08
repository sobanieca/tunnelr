import { parseArgs } from "jsr:@std/cli@1.0.32/parse-args";
import {
  bold,
  brightBlue,
  brightRed,
  brightYellow,
  gray,
} from "jsr:@std/fmt@1.0.10/colors";
import { dirname, join } from "jsr:@std/path@1.1.6";

const deps = {
  parse: parseArgs,
  colors: { bold, brightBlue, brightRed, brightYellow, gray },
  dirname,
  join,
};

export { deps };
