// Subject-hash vectors from NFS-World-Offline's calculateHashFromData.js:
//   node reference/nfswo/subject_vectors.js > fixtures/vectors/xmpp_subject.json
// nfswo hashes the recipient JID followed by the body, before xml2js escapes it.
"use strict";
const path = require("path");

const nfswo = path.join(__dirname, "../../upstream/NFS-World-Offline");
const calculateHashFromData = require(path.join(nfswo, "src/utils/calculateHashFromData.js"));

// The core's XmppChat.createSystemMessage, as SBRW pushed it.
const systemMessage = (message) =>
    "<response status='1' ticket='0'>\n<ChatBroadcast>\n<ChatBlob>\n<FromName>System</FromName>\n" +
    "<FromPersonaId>0</FromPersonaId>\n<FromUserId>0</FromUserId>\n<Message>" + message +
    "</Message>\n<ToId>0</ToId>\n<Type>2</Type>\n</ChatBlob>\n</ChatBroadcast>\n</response>";

// What nfswo itself sends after login.
const nfswoWelcome = "<response status=\"1\" ticket=\"0\"><ChatBroadcast><ChatBlob><FromName>System</FromName>" +
    "<FromPersonaId>0</FromPersonaId><FromUserId>0</FromUserId><Message>----------------------------------------------------\n" +
    "Welcome to NFS World Offline by Lawin!\n----------------------------------------------------</Message>" +
    "<ToId>0</ToId><Type>2</Type></ChatBlob></ChatBroadcast></response>";

// A fixed LCG, so the long cases are the same on every run.
let seed = 20261014;
const next = () => (seed = (Math.imul(seed, 1103515245) + 12345) >>> 0);
const generated = (len, maxCode) => {
    let s = "";
    for (let i = 0; i < len; i++) s += String.fromCharCode(next() % maxCode);
    return s;
};

const cases = [
    "",
    "a",
    "A",
    "\u0000",
    "￿",
    "sbrw.100@127.0.0.1",
    "nfsw.100@localhost" + nfswoWelcome,
    "sbrw.100@127.0.0.1" + systemMessage("Welcome!"),
    "sbrw.123456@203.0.113.7" + systemMessage("café 中文"),
    "sbrw.100@127.0.0.1" + systemMessage("😀 non-BMP"),
    "\ud800",
    "\udfff\ud800",
    "x".repeat(1000),
    "￿".repeat(1000),
    generated(64, 0x80),
    generated(257, 0x10000),
    generated(4096, 0x10000),
];

const vectors = cases.map((data) => [
    Array.from({ length: data.length }, (_, i) => data.charCodeAt(i).toString(16).padStart(4, "0")).join(""),
    calculateHashFromData(data),
]);

process.stdout.write(JSON.stringify({
    about: `calculateHashFromData.js from NFS-World-Offline on node ${process.version}: [UTF-16 code units as hex, subject]`,
    vectors,
}, null, 2) + "\n");
