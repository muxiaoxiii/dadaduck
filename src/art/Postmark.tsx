/** A two-ink duck stamp shared by the wordmark and the airmail envelope. */
export function DuckMark() {
  return <svg aria-hidden="true" viewBox="0 0 64 64" fill="none">
    <path d="M8 34q8 5 17 3c-3-4-4-9-2-14 3-9 13-12 21-8 7 3 9 11 7 17 4 4 4 11 0 16-5 7-14 9-23 8C16 54 9 46 8 34Z" fill="#F4CE64" stroke="#193E33" strokeWidth="2.4" strokeLinejoin="round"/>
    <path d="M48 27c5 1 9 0 12 2-1 4-6 6-12 6l-4-3Z" fill="#DE8944" stroke="#193E33" strokeWidth="1.8"/>
    <circle cx="43.7" cy="24.4" r="2.1" fill="#173E32"/>
    <path d="M18 40c6 4 14 3 17-1-1 9-11 12-17 1Z" fill="#E8B847" stroke="#193E33" strokeWidth="1.7" strokeLinejoin="round"/>
  </svg>;
}

export function Airmail() {
  return <svg className="airmail" viewBox="0 0 330 146" fill="none" aria-hidden="true">
    <defs>
      <pattern id="air-stripes" width="27" height="14" patternUnits="userSpaceOnUse" patternTransform="rotate(36)">
        <rect width="27" height="14" fill="#F3ECCA"/><rect width="8" height="14" fill="#C3654B"/><rect x="14" width="8" height="14" fill="#427263"/>
      </pattern>
      <pattern id="paper-grain" width="5" height="5" patternUnits="userSpaceOnUse"><circle cx="1" cy="1" r=".55" fill="#967F4A" opacity=".36"/></pattern>
    </defs>
    <path d="M-10 106q40 9 57-13c17-22-14-25-12-11s39 26 73-5" stroke="#A4BDA3" strokeWidth="1.3" strokeDasharray="4 6"/>
    <g transform="rotate(-9 191 80)">
      <rect x="91" y="25" width="191" height="110" rx="4" fill="#112E26" opacity=".45"/>
      <rect x="88" y="19" width="191" height="110" rx="4" fill="url(#air-stripes)"/>
      <rect x="93" y="24" width="181" height="100" rx="1" fill="#F4EDD3"/>
      <rect x="93" y="24" width="181" height="100" fill="url(#paper-grain)"/>
      <g stroke="#8C9E83" strokeWidth="2"><path d="M108 90h73m-73 7h56m-56 7h33"/></g>
      <path d="M108 37h45v14h-45z" stroke="#6F8E76"/>
      <text x="113" y="47" fontFamily="monospace" fontSize="8" fontWeight="bold" letterSpacing="1" fill="#487259">AIR MAIL</text>
      <g transform="translate(218 31) rotate(5 22 25)">
        <rect width="48" height="56" fill="#F4C65C" stroke="#F4EDD3" strokeWidth="5" strokeDasharray="3 3"/>
        <path d="M4 4h40v48H4z" stroke="#6F753D"/>
        <svg x="6" y="6" width="35" height="35" viewBox="0 0 64 64"><DuckMark/></svg>
        <text x="24" y="47" textAnchor="middle" fontFamily="monospace" fontSize="6.5" fontWeight="bold" letterSpacing=".7" fill="#364E37">DADADUCK</text>
      </g>
      <g transform="translate(208 75) rotate(-10)" stroke="#A15F49" opacity=".8">
        <circle r="20" strokeWidth="1.7"/><circle r="16" strokeWidth=".7"/>
        <path d="M-12-4h24m-23 8h22m11-10c9-5 16 4 29-1m-29 7c9-5 16 4 29-1m-29 7c9-5 16 4 29-1" strokeWidth="1.3"/>
        <text y="2" textAnchor="middle" fill="#A15F49" stroke="none" fontSize="5.3" fontFamily="monospace">D · D · POST</text>
      </g>
    </g>
  </svg>;
}
