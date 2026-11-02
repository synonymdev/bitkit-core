"""Run after ./build.sh python with PYTHONPATH=bindings/python."""

from bitkitcore import (
    TrezorGetAddressParams,
    TrezorGetPublicKeyParams,
    TrezorPublicKeyResponse,
    TrezorSignMessageParams,
)

path = "m/84'/0'/0'"
requests = [
    TrezorGetAddressParams(
        path=path, coin=None, show_on_trezor=False, script_type=None
    ),
    TrezorGetPublicKeyParams(path=path, coin=None, show_on_trezor=False),
    TrezorSignMessageParams(path=path, message="test", coin=None),
]
assert all(request.cross_chain is False for request in requests)
path_override = TrezorGetPublicKeyParams(
    path=path, coin=None, show_on_trezor=False, cross_chain=True
)
assert path_override.cross_chain is True

response = TrezorPublicKeyResponse(
    xpub="xpub", xpub_segwit="zpub", descriptor=None,
    displayable_public_key="zpub", path=path, public_key="02",
    chain_code="00", fingerprint=42, depth=3, root_fingerprint=0x73C5DA0A,
)
assert response.xpub == "xpub"
assert response.xpub_segwit == response.displayable_public_key == "zpub"
assert response.descriptor is None
assert response.root_fingerprint == 0x73C5DA0A
print("Python Trezor record smoke test passed")
