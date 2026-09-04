# EIP-3009 createAndFund 排查 Demo

这个 Node.js demo 使用 SWIMLANE 失败请求中的真实参数，复现以下计算：

1. 对 `hookData` 计算 `keccak256`。
2. 按 AgentPayment 的 15 个字段执行 `abi.encode`。
3. 计算 escrow `nonce/orderId`。
4. 计算 `ReceiveWithAuthorization` struct hash。
5. 使用 response 中的 `domainHash` 计算最终 EIP-712 digest。
6. 从 signature recover Buyer 地址。
7. 额外计算错误使用 `taskSalt` 时的 nonce，方便与后端结果比较。

## 运行环境

- Node.js 18 或以上
- npm

## 运行

```bash
cd examples/eip3009-create-and-fund
npm install
npm start
```

成功时最后会输出：

```text
PASS escrow nonce: 0x07fb87a08a2c001e791604e810d447cf42a1e4df027c3219393a2ea510459ac1
PASS Receive structHash: 0xcc0de4351fb5132b9dd68fa535c7b2c3e05a4f4854941b7e067e45e099950625
PASS EIP-712 digest: 0xb101dfc1fb48109d5926cc9b4139f75b660fb7e58ceff4a168879920eb471277
PASS recovered signer: 0x1749b518cd0789c46dF308CDaD5B59372e223D79

All checks passed. The sample signature is cryptographically valid.
```

## 后端核对方式

请将后端在 `createAndFund` 中实际计算的 `orderId`，以及最终传给 USDT
`receiveWithAuthorization` 的参数，与 demo 输出逐项比较。

正确的 `orderId` 应为：

```text
0x07fb87a08a2c001e791604e810d447cf42a1e4df027c3219393a2ea510459ac1
```

如果后端输出等于 demo 的 `nonce using taskSalt`，表示后端错误地将
`taskSalt` 当成了 escrow `salt`。

要验证其他失败请求，直接替换 `index.mjs` 顶部的 `sample` 和 `expected` 数据。
