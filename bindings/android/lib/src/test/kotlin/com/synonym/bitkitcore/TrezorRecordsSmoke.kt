package com.synonym.bitkitcore

// Compiled by :lib:compileDebugUnitTestKotlin after regenerating bindings.
internal fun trezorRecordsSmoke() {
    val path = "m/84'/0'/0'"
    val address = TrezorGetAddressParams(
        path = path, coin = null, showOnTrezor = false, scriptType = null,
    )
    val key = TrezorGetPublicKeyParams(path = path, coin = null, showOnTrezor = false)
    val message = TrezorSignMessageParams(path = path, message = "test", coin = null)
    check(!address.crossChain && !key.crossChain && !message.crossChain)
    val pathOverride = TrezorGetPublicKeyParams(
        path = path, coin = null, showOnTrezor = false, crossChain = true,
    )
    check(pathOverride.crossChain)
    val response = TrezorPublicKeyResponse(
        xpub = "xpub", xpubSegwit = "zpub", descriptor = null,
        displayablePublicKey = "zpub", path = path, publicKey = "02",
        chainCode = "00", fingerprint = 42u, depth = 3u, rootFingerprint = 0x73c5da0au,
    )
    check(response.xpub == "xpub")
    check(response.xpubSegwit == response.displayablePublicKey)
    check(response.descriptor == null)
    check(response.rootFingerprint == 0x73c5da0au)
}
