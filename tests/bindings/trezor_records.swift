// Compile with the generated Swift bindings and bitkitcoreFFI module.
import Foundation

@main
struct TrezorRecordsSmoke {
    static func main() {
        let path = "m/84'/0'/0'"
        let address = TrezorGetAddressParams(
            path: path, coin: nil, showOnTrezor: false, scriptType: nil
        )
        let key = TrezorGetPublicKeyParams(path: path, coin: nil, showOnTrezor: false)
        let message = TrezorSignMessageParams(path: path, message: "test", coin: nil)
        precondition(!address.crossChain && !key.crossChain && !message.crossChain)
        let pathOverride = TrezorGetPublicKeyParams(
            path: path, coin: nil, showOnTrezor: false, crossChain: true
        )
        precondition(pathOverride.crossChain)
        let response = TrezorPublicKeyResponse(
            xpub: "xpub", xpubSegwit: "zpub", descriptor: nil,
            displayablePublicKey: "zpub", path: path, publicKey: "02",
            chainCode: "00", fingerprint: 42, depth: 3, rootFingerprint: 0x73c5da0a
        )
        precondition(response.xpub == "xpub")
        precondition(response.xpubSegwit == response.displayablePublicKey)
        precondition(response.descriptor == nil)
        precondition(response.rootFingerprint == 0x73c5da0a)
        print("Swift Trezor record smoke test passed")
    }
}
