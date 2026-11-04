# Finding games on the same Wi-Fi on iPhone and iPad

Ourcraft finds games on the local network with a UDP broadcast: a game
that's hosting sends "I'm here!" to port 41701 once a second, and the
**Join a friend** screen listens for it (`crates/server/src/net.rs`,
`crates/client/src/net.rs`).

On iOS 14 and later, an app may only send or receive broadcast and
multicast packets if it has Apple's
`com.apple.developer.networking.multicast` entitlement. Without it, an
iPhone or iPad can't see games on the Wi-Fi, or be seen. Typing the other
device's address still works. The entitlement isn't automatic: the
account holder has to ask Apple for it.

## 1. Ask Apple (the account holder does this)

Go to https://developer.apple.com/contact/request/networking-multicast,
sign in with the Apple developer account, and fill in the form. The
answers below are ready to paste.

- **App name:** Ourcraft
- **Bundle ID:** `org.ourcraft.game`
- **App Store / TestFlight link:** (if there is one)
- **Why the app needs multicast networking:**

  > Ourcraft is a free, open-source building game for children
  > (https://github.com/rsantacroce/ourcraft). Players on the same Wi-Fi,
  > such as children at home or in a classroom, play together in one
  > world. One device hosts the game, and to let the others find it
  > without typing an IP address, the host sends a small UDP broadcast
  > (a few hundred bytes at most, once per second, to 255.255.255.255
  > port 41701) with the game's name, welcome message, port and number of
  > players. The other devices listen on that port
  > and list the games they hear. Nothing is sent outside the local
  > network, and no data about the user is in the packet. We already ask
  > for local network access (NSLocalNetworkUsageDescription) and declare
  > `_ourcraft._udp` in NSBonjourServices.

- **Protocols and ports:** UDP broadcast to 255.255.255.255, port 41701;
  the game itself then uses UDP port 41700 (unicast).

Apple usually answers by email within a few days to a couple of weeks.

## 2. When Apple says yes

1. In the developer account, under **Certificates, Identifiers &
   Profiles → Identifiers → org.ourcraft.game**, tick **Multicast
   Networking** in the capabilities list (it only shows up once granted)
   and save. Regenerate the provisioning profile if you manage it by hand.
2. Add an entitlements file, `mobile/ios/ios-src/Ourcraft.entitlements`:

   ```xml
   <?xml version="1.0" encoding="UTF-8"?>
   <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
   <plist version="1.0">
   <dict>
   	<key>com.apple.developer.networking.multicast</key>
   	<true/>
   </dict>
   </plist>
   ```

3. In Xcode, select the target, **Build Settings → Code Signing
   Entitlements**, and set it to `ios-src/Ourcraft.entitlements` (this is
   `CODE_SIGN_ENTITLEMENTS` in `project.pbxproj`). Or add **Multicast
   Networking** under **Signing & Capabilities**, which does the same.
4. Build to a real device, host a world on one device, and check that
   **Join a friend** lists it on another.

Don't add the entitlement before Apple grants it: signing fails, because
the provisioning profile doesn't include it.
