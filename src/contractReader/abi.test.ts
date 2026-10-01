// HUP-S6.7 — the Contract reader's ABI handling: parse an ABI (verified or pasted), turn the
// member's typed inputs into calldata, and decode what a read returns.
import { describe, expect, it } from "vitest";
import { decodeResult, encodeCall, parseAbi, parseArg } from "./abi";

const ERC721_BITS = [
  { type: "function", name: "name", inputs: [], outputs: [{ name: "", type: "string" }], stateMutability: "view" },
  { type: "function", name: "totalSupply", inputs: [], outputs: [{ name: "", type: "uint256" }], stateMutability: "view" },
  { type: "function", name: "balanceOf", inputs: [{ name: "owner", type: "address" }], outputs: [{ name: "", type: "uint256" }], stateMutability: "view" },
  { type: "function", name: "mint", inputs: [{ name: "quantity", type: "uint256" }], outputs: [], stateMutability: "payable" },
  { type: "function", name: "withdraw", inputs: [], outputs: [], stateMutability: "nonpayable" },
  { type: "event", name: "Transfer", inputs: [] },
  { type: "constructor", inputs: [] },
];

describe("parseAbi", () => {
  it("keeps functions, classifies reads and writes, and sorts reads first", () => {
    const r = parseAbi(ERC721_BITS);
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect(r.functions.map((f) => [f.name, f.kind])).toEqual([
      ["balanceOf", "read"],
      ["name", "read"],
      ["totalSupply", "read"],
      ["mint", "write"],
      ["withdraw", "write"],
    ]);
    const mint = r.functions.find((f) => f.name === "mint");
    expect(mint?.payable).toBe(true);
    expect(mint?.signature).toBe("mint(uint256)");
    expect(mint?.selector).toBe("0xa0712d68");
  });

  it("accepts pasted JSON text and the legacy constant flag", () => {
    const r = parseAbi(JSON.stringify([{ type: "function", name: "owner", inputs: [], outputs: [{ type: "address" }], constant: true }]));
    expect(r.ok && r.functions[0].kind).toBe("read");
  });

  it("refuses junk instead of guessing", () => {
    for (const bad of ["not json", "{}", "[1,2]", JSON.stringify([{ type: "function", name: "x; y", inputs: [] }]), "[]"]) {
      const r = parseAbi(bad);
      expect(r.ok, bad).toBe(false);
    }
  });

  it("keeps overloads apart by signature", () => {
    const r = parseAbi([
      { type: "function", name: "safeTransferFrom", inputs: [{ type: "address" }, { type: "address" }, { type: "uint256" }], outputs: [], stateMutability: "nonpayable" },
      { type: "function", name: "safeTransferFrom", inputs: [{ type: "address" }, { type: "address" }, { type: "uint256" }, { type: "bytes" }], outputs: [], stateMutability: "nonpayable" },
    ]);
    expect(r.ok && r.functions.map((f) => f.signature)).toEqual([
      "safeTransferFrom(address,address,uint256)",
      "safeTransferFrom(address,address,uint256,bytes)",
    ]);
  });
});

describe("parseArg", () => {
  it("parses the common types from text", () => {
    expect(parseArg({ type: "uint256" }, "500")).toBe(500n);
    expect(parseArg({ type: "uint256" }, "0x1f4")).toBe(500n);
    expect(parseArg({ type: "int8" }, "-3")).toBe(-3n);
    expect(parseArg({ type: "bool" }, "true")).toBe(true);
    expect(parseArg({ type: "bool" }, "false")).toBe(false);
    expect(parseArg({ type: "address" }, " 0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed ")).toBe("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
    expect(parseArg({ type: "bytes" }, "0xdeadbeef")).toBe("0xdeadbeef");
    expect(parseArg({ type: "string" }, "Lemon Drops")).toBe("Lemon Drops");
    expect(parseArg({ type: "uint256[]" }, "[1, \"2\"]")).toEqual([1n, 2n]);
    expect(parseArg({ type: "tuple", components: [{ name: "a", type: "uint8" }, { name: "b", type: "bool" }] }, "[7, true]")).toEqual({ a: 7n, b: true });
  });

  it("names what is wrong with a bad value", () => {
    expect(() => parseArg({ type: "uint256" }, "1.5")).toThrow(/whole number/);
    expect(() => parseArg({ type: "uint256" }, "-1")).toThrow(/negative/);
    expect(() => parseArg({ type: "bool" }, "yes")).toThrow(/true or false/);
    expect(() => parseArg({ type: "address" }, "0x123")).toThrow(/address/);
    expect(() => parseArg({ type: "bytes4" }, "0x12")).toThrow(/4 bytes/);
    expect(() => parseArg({ type: "uint256[]" }, "1,2")).toThrow(/JSON array/);
  });
});

describe("encodeCall / decodeResult", () => {
  it("encodes a call the chain understands and decodes its answer", () => {
    const r = parseAbi(ERC721_BITS);
    if (!r.ok) throw new Error(r.error);
    const balanceOf = r.functions.find((f) => f.name === "balanceOf")!;
    const data = encodeCall(balanceOf, ["0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"]);
    expect(data).toBe("0x70a082310000000000000000000000005aaeb6053f3e94c9b9a09f33669435e7ef1beaed");
    expect(decodeResult(balanceOf, "0x" + "0".repeat(62) + "03")).toEqual(["3"]);

    const name = r.functions.find((f) => f.name === "name")!;
    expect(encodeCall(name, [])).toBe("0x06fdde03");
    const encoded =
      "0x" +
      "0000000000000000000000000000000000000000000000000000000000000020" +
      "000000000000000000000000000000000000000000000000000000000000000b" +
      Buffer.from("Lemon Drops").toString("hex").padEnd(64, "0");
    expect(decodeResult(name, encoded)).toEqual(["Lemon Drops"]);
  });

  it("refuses the wrong number of inputs", () => {
    const r = parseAbi(ERC721_BITS);
    if (!r.ok) throw new Error(r.error);
    const mint = r.functions.find((f) => f.name === "mint")!;
    expect(() => encodeCall(mint, [])).toThrow(/1 input/);
  });

  it("an empty answer from a read is reported, not decoded into zeros", () => {
    const r = parseAbi(ERC721_BITS);
    if (!r.ok) throw new Error(r.error);
    const total = r.functions.find((f) => f.name === "totalSupply")!;
    expect(() => decodeResult(total, "0x")).toThrow(/returned nothing/);
  });
});
