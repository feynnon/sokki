/**
 * "Email me the setup": the email a person sends themselves from one device
 * so they set up Sokki on their other one. Used by the Mac app (Phone
 * settings) and the Android app (Connect screen). It only opens the
 * person's own mail app with this text filled in; Sokki sends nothing.
 *
 * Edit the wording here; both apps pick it up at the next build.
 */

export const INSTALL = {
  repo: "https://github.com/feynnon/sokki",
  releases: "https://github.com/feynnon/sokki/releases/latest",
  macCommand:
    "curl -fsSL https://raw.githubusercontent.com/feynnon/sokki/main/install.sh | bash",
};

export function inviteEmail(from: "mac" | "android"): {
  subject: string;
  body: string;
} {
  const here = from === "mac" ? "your Mac" : "your phone";
  const body = `You set up Sokki on ${here}. Install it on your other devices too, so meetings you record on your phone become notes on your Mac.

MAC (Apple Silicon, macOS 14.2 or later)
1. Open Terminal and paste this line:
   ${INSTALL.macCommand}
2. In Sokki: Meetings → settings → Phone → switch on "Accept recordings from my phone". A QR code appears.

ANDROID PHONE
1. Download the app on your phone: ${INSTALL.releases} (the file ending in .apk), open it and allow the install.
2. In the app, tap "Scan the code" and scan the QR code on your Mac.

Phone and Mac don't need to be on the same network. The Mac only has to be on when you send a recording.

More: ${INSTALL.repo}#install
`;
  return { subject: "Set up Sokki on your other devices", body };
}

/** A mailto: link that opens the person's mail app with the email filled in.
 * The recipient stays empty: they put in their own address. */
export function inviteMailto(from: "mac" | "android"): string {
  const { subject, body } = inviteEmail(from);
  return `mailto:?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(body)}`;
}
