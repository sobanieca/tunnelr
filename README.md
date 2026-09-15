# tunnelr

Expose ports of your local machine to the internet through a cheap VPS.

```
internet  -->  my-vps.example.com:30185  ==tunnel==>  your machine:3000
```

## Server

1. Buy a cheap VPS, for example [mikr.us](https://mikr.us) (less than 10 USD per
   year!).

   > Note: Whatever server you decide to buy it is recommended to use Debian OS.

2. Log in to the VPS as the provider describes, then:

   a. Install `tunnelr`:

   ```bash
   curl -fsSL sobanieca.github.io/tunnelr/install.sh | bash
   ```

   b. Check in the provider's panel which ports the VPS has open (cheap
   providers forward only a few, mikr.us gives two, like `20185` and `30185`)
   and start tunnelr on one of them:

   ```bash
   sudo tunnelr -p 20185
   ```

   It prints the address and the key:

   ```
   tunnelr server 0.1.0 - systemd service "tunnelr" installed and started

     Address:   203.0.113.10:20185
     Key:       kD3xW9q1mZ8pR4tY7uH2cV6bN0aS5fGj
     Key file:  /root/.secret/tunnelr-key

   Run this command again anytime to see the address and the key.
   ```

Congratulations, you have your own tunnel server! It survives reboots.

## Your machine

3. Install `tunnelr`:

   ```bash
   curl -fsSL sobanieca.github.io/tunnelr/install.sh | bash
   ```

4. Save the key printed by the server in a file, for example
   `~/.secret/tunnelr-key`.
5. Open a port of your machine to the world, using another open port of the VPS
   (use the address printed by the server):

   ```bash
   tunnelr 203.0.113.10:20185 -p 30185:3000 -a ~/.secret/tunnelr-key
   ```

Now `203.0.113.10:30185` reaches port 3000 on your machine. Keep the command
running, `Ctrl+C` closes the tunnel.

## Good to know

- `-a` takes the key itself or a path to a file with it. Without `-a` tunnelr
  reads `~/.secret/tunnelr-key`.
- The key never travels over the network. Every request carries a one-time token
  signed with the key, so a captured token cannot be reused.
- `-p 30185:3000` forwards VPS port 30185 to local port 3000. When the VPS has
  all ports open, `-p 3000,4000` opens the same ports on both sides.
  `tunnelr --help` shows all options and the HTTP API.
- Every port tunnelr uses (the control port and each exposed port) has to be one
  the VPS really has open, so two open ports mean one tunnel.
- tunnelr does not encrypt the traffic. Use HTTPS or SSH inside the tunnel when
  the data is sensitive.
- Update: run the install command again
