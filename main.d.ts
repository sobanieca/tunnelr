/**
 * tunnelr - expose ports of your local machine through a cheap VPS.
 *
 * Run the server on the VPS and the client on your machine, on ports the VPS
 * has open:
 *
 * @example
 * ```bash
 * # On the VPS
 * tunnelr -p 20185
 *
 * # On your machine
 * tunnelr my-vps.example.com:20185 -p 30185:3000
 * ```
 */

/**
 * No exports - this module executes commands when imported/executed.
 */
export {};
