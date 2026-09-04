import {
  AbiCoder,
  Signature,
  TypedDataEncoder,
  concat,
  getAddress,
  keccak256,
  recoverAddress,
} from "ethers";

// SWIMLANE createAndFundConfirmStatus response + wallet signing response.
// Replace these values with another failed request to reproduce that request.
const sample = {
  escrow: {
    from: "0x1749b518cd0789c46df308cdad5b59372e223d79",
    provider: "0x1749b518cd0789c46df308cdad5b59372e223d79",
    receiver: "0x1749b518cd0789c46df308cdad5b59372e223d79",
    arbitrator: "0x33a3df7e057d9c8be39b5947efbe740a11f83a84",
    currency: "0x779ded0c9e1022225f8e0630b35a9b54be713736",
    amount: 100000n,
    submitWindow: 1200n,
    disputeWindow: 900n,
    arbitrationWindow: 18446744073709551615n,
    terminationWindow: 86400n,
    hook: "0x64fa57590af29aa188068e7a9cc64c903adf578b",
    hookData:
      "0xcda9e0af098ca95a9f343f9bb488058dae54b1cf3e8ac20e47fbd51ba3c26477",
    salt:
      "0xcda9e0af098ca95a9f343f9bb488058dae54b1cf3e8ac20e47fbd51ba3c26477",
    taskSalt:
      "0xfe88c45c65dbe0d3f2c5c6679abb6276de45605dee68412bc8882bc07ed0ea8e",
    chainId: 196n,
    escrowAddress: "0x00e5c0051cd65f261720e0bdcf459e136552dbf5",
  },
  authorization: {
    from: "0x1749b518cd0789c46df308cdad5b59372e223d79",
    to: "0x00e5c0051cd65f261720e0bdcf459e136552dbf5",
    value: 100000n,
    validAfter: 0n,
    validBefore: 1788320927n,
  },
  domainSeparator:
    "0xd591d9baf744328d9400b923cb02c9474d367d591ca1ab24d8c4068be527599d",
  signature:
    "0x80cbeb045b897bcf031bc7b35fd533827d20de80a53ea79964633a6cb5eacac816ef855ffbb69a4a0025240dab9774346ce15cf7e49680e6f7fe65a1dab8fc141c",
};

const expected = {
  nonce:
    "0x07fb87a08a2c001e791604e810d447cf42a1e4df027c3219393a2ea510459ac1",
  structHash:
    "0xcc0de4351fb5132b9dd68fa535c7b2c3e05a4f4854941b7e067e45e099950625",
  digest:
    "0xb101dfc1fb48109d5926cc9b4139f75b660fb7e58ceff4a168879920eb471277",
  signer: "0x1749b518cd0789c46df308cdad5b59372e223d79",
};

const escrowTypes = [
  "address",
  "address",
  "address",
  "address",
  "address",
  "uint256",
  "uint64",
  "uint64",
  "uint64",
  "uint64",
  "address",
  "bytes32",
  "bytes32",
  "uint256",
  "address",
];

const receiveWithAuthorizationTypes = {
  ReceiveWithAuthorization: [
    { name: "from", type: "address" },
    { name: "to", type: "address" },
    { name: "value", type: "uint256" },
    { name: "validAfter", type: "uint256" },
    { name: "validBefore", type: "uint256" },
    { name: "nonce", type: "bytes32" },
  ],
};

function calculateEscrowNonce(escrow, salt = escrow.salt) {
  const hookDataHash = keccak256(escrow.hookData);
  const encoded = AbiCoder.defaultAbiCoder().encode(escrowTypes, [
    escrow.from,
    escrow.provider,
    escrow.receiver,
    escrow.arbitrator,
    escrow.currency,
    escrow.amount,
    escrow.submitWindow,
    escrow.disputeWindow,
    escrow.arbitrationWindow,
    escrow.terminationWindow,
    escrow.hook,
    hookDataHash,
    salt,
    escrow.chainId,
    escrow.escrowAddress,
  ]);

  return {
    hookDataHash,
    encoded,
    nonce: keccak256(encoded),
  };
}

function assertEqual(label, actual, wanted) {
  if (actual.toLowerCase() !== wanted.toLowerCase()) {
    throw new Error(`${label} mismatch\n  actual:   ${actual}\n  expected: ${wanted}`);
  }
  console.log(`PASS ${label}: ${actual}`);
}

const correct = calculateEscrowNonce(sample.escrow);
const wrongTaskSalt = calculateEscrowNonce(
  sample.escrow,
  sample.escrow.taskSalt,
);

const authorization = {
  ...sample.authorization,
  nonce: correct.nonce,
};

const structHash = TypedDataEncoder.hashStruct(
  "ReceiveWithAuthorization",
  receiveWithAuthorizationTypes,
  authorization,
);

// EIP-712 digest = keccak256(0x1901 || domainSeparator || structHash).
// domainSeparator comes from the Wallet API response and can also be checked
// against USDT.DOMAIN_SEPARATOR() on X Layer.
const digest = keccak256(
  concat(["0x1901", sample.domainSeparator, structHash]),
);
const recoveredSigner = recoverAddress(digest, sample.signature);
const parsedSignature = Signature.from(sample.signature);

console.log("SWIMLANE EIP-3009 createAndFund debug demo\n");
console.log(`hookDataHash:             ${correct.hookDataHash}`);
console.log(`ABI encoded bytes:        ${(correct.encoded.length - 2) / 2}`);
console.log(`escrow nonce/orderId:     ${correct.nonce}`);
console.log(`nonce using taskSalt:     ${wrongTaskSalt.nonce}`);
console.log(`Receive structHash:       ${structHash}`);
console.log(`EIP-712 domainSeparator:  ${sample.domainSeparator}`);
console.log(`EIP-712 digest:           ${digest}`);
console.log(`recovered signer:         ${recoveredSigner}`);
console.log(`signature r:              ${parsedSignature.r}`);
console.log(`signature s:              ${parsedSignature.s}`);
console.log(`signature v:              ${parsedSignature.v}\n`);

assertEqual("escrow nonce", correct.nonce, expected.nonce);
assertEqual("Receive structHash", structHash, expected.structHash);
assertEqual("EIP-712 digest", digest, expected.digest);
assertEqual(
  "recovered signer",
  getAddress(recoveredSigner),
  getAddress(expected.signer),
);

if (correct.nonce.toLowerCase() === wrongTaskSalt.nonce.toLowerCase()) {
  throw new Error("salt and taskSalt unexpectedly produced the same nonce");
}

console.log("\nAll checks passed. The sample signature is cryptographically valid.");
console.log(
  "Compare the backend's actual AgentPayment calldata and computed orderId with the values above.",
);
