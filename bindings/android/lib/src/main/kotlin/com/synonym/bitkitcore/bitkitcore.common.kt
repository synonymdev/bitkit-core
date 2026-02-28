

@file:Suppress("RemoveRedundantBackticks")

package com.synonym.bitkitcore

// Common helper code.
//
// Ideally this would live in a separate .kt file where it can be unittested etc
// in isolation, and perhaps even published as a re-useable package.
//
// However, it's important that the details of how this helper code works (e.g. the
// way that different builtin types are passed across the FFI) exactly match what's
// expected by the Rust code on the other side of the interface. In practice right
// now that means coming from the exact some version of `uniffi` that was used to
// compile the Rust component. The easiest way to ensure this is to bundle the Kotlin
// helpers directly inline like we're doing here.

public class InternalException(message: String) : kotlin.Exception(message)

// Public interface members begin here.


// Interface implemented by anything that can contain an object reference.
//
// Such types expose a `destroy()` method that must be called to cleanly
// dispose of the contained objects. Failure to call this method may result
// in memory leaks.
//
// The easiest way to ensure this method is called is to use the `.use`
// helper method to execute a block and destroy the object at the end.
@OptIn(ExperimentalStdlibApi::class)
public interface Disposable : AutoCloseable {
    public fun destroy()
    override fun close(): Unit = destroy()
    public companion object {
        internal fun destroy(vararg args: Any?) {
            for (arg in args) {
                when (arg) {
                    is Disposable -> arg.destroy()
                    is ArrayList<*> -> {
                        for (idx in arg.indices) {
                            val element = arg[idx]
                            if (element is Disposable) {
                                element.destroy()
                            }
                        }
                    }
                    is Map<*, *> -> {
                        for (element in arg.values) {
                            if (element is Disposable) {
                                element.destroy()
                            }
                        }
                    }
                    is Array<*> -> {
                        for (element in arg) {
                            if (element is Disposable) {
                                element.destroy()
                            }
                        }
                    }
                    is Iterable<*> -> {
                        for (element in arg) {
                            if (element is Disposable) {
                                element.destroy()
                            }
                        }
                    }
                }
            }
        }
    }
}

@OptIn(kotlin.contracts.ExperimentalContracts::class)
public inline fun <T : Disposable?, R> T.use(block: (T) -> R): R {
    kotlin.contracts.contract {
        callsInPlace(block, kotlin.contracts.InvocationKind.EXACTLY_ONCE)
    }
    return try {
        block(this)
    } finally {
        try {
            // N.B. our implementation is on the nullable type `Disposable?`.
            this?.destroy()
        } catch (e: Throwable) {
            // swallow
        }
    }
}

/** Used to instantiate an interface without an actual pointer, for fakes in tests, mostly. */
public object NoPointer



















/**
 * Callback interface for native Trezor transport operations
 *
 * This trait must be implemented by the native iOS/Android code.
 * The implementation handles actual USB or Bluetooth communication.
 *
 * # Android Implementation
 * Use Android USB Host API for USB devices:
 * - Enumerate devices with vendorId 0x1209 (0x534c for older), productId 0x53c1
 * - Request USB permission, claim interface, get endpoints
 * - Chunk size: 64 bytes for USB
 *
 * Use Android BLE API for Bluetooth:
 * - Scan for Trezor BLE service UUID: 8c000001-a59b-4d58-a9ad-073df69fa1b1
 * - Connect and discover characteristics
 * - Read from: 8c000002-a59b-4d58-a9ad-073df69fa1b1
 * - Write to: 8c000003-a59b-4d58-a9ad-073df69fa1b1
 * - Chunk size: 244 bytes for BLE
 *
 * # iOS Implementation
 * Use IOKit/CoreBluetooth with same service/characteristic UUIDs.
 */
public interface TrezorTransportCallback {
    
    /**
     * Enumerate all connected Trezor devices
     */
    public fun `enumerateDevices`(): List<NativeDeviceInfo>
    
    /**
     * Open a connection to a device
     */
    public fun `openDevice`(`path`: kotlin.String): TrezorTransportWriteResult
    
    /**
     * Close the connection to a device
     */
    public fun `closeDevice`(`path`: kotlin.String): TrezorTransportWriteResult
    
    /**
     * Read a chunk of data from the device
     */
    public fun `readChunk`(`path`: kotlin.String): TrezorTransportReadResult
    
    /**
     * Write a chunk of data to the device
     */
    public fun `writeChunk`(`path`: kotlin.String, `data`: kotlin.ByteArray): TrezorTransportWriteResult
    
    /**
     * Get the chunk size for a device (64 for USB, 244 for Bluetooth)
     */
    public fun `getChunkSize`(`path`: kotlin.String): kotlin.UInt
    
    /**
     * High-level message call for BLE/THP devices.
     *
     * For BLE devices that use THP protocol (encrypted communication),
     * the native layer should handle encryption/decryption via
     * android-trezor-connect and return the raw protobuf response.
     *
     * Returns None if not supported (will fall back to Protocol V1 chunks).
     * Returns Some(result) to use native THP handling.
     *
     * # Arguments
     * * `path` - Device path
     * * `message_type` - Protobuf message type (e.g., GetAddress = 29)
     * * `data` - Serialized protobuf message data
     */
    public fun `callMessage`(`path`: kotlin.String, `messageType`: kotlin.UShort, `data`: kotlin.ByteArray): TrezorCallMessageResult?
    
    /**
     * Get pairing code from user during BLE THP pairing.
     *
     * This is called when the Trezor device displays a 6-digit code
     * that must be entered to complete Bluetooth pairing.
     *
     * The native layer should display a UI for the user to enter the code
     * shown on the Trezor screen.
     *
     * Returns the 6-digit code as a string, or empty string to cancel.
     */
    public fun `getPairingCode`(): kotlin.String
    
    /**
     * Save THP pairing credentials for a device.
     *
     * Called after successful BLE pairing to store credentials for reconnection.
     * The credential_json is a JSON string containing the serialized ThpCredentials.
     *
     * # Arguments
     * * `device_id` - Device identifier (e.g., BLE address like "ble:AA:BB:CC:DD:EE:FF")
     * * `credential_json` - JSON string with credential data
     *
     * Returns true if credentials were saved successfully.
     */
    public fun `saveThpCredential`(`deviceId`: kotlin.String, `credentialJson`: kotlin.String): kotlin.Boolean
    
    /**
     * Load THP pairing credentials for a device.
     *
     * Called before BLE handshake to check for stored credentials.
     * If credentials are found, they will be used to skip the pairing dialog.
     *
     * # Arguments
     * * `device_id` - Device identifier (e.g., BLE address like "ble:AA:BB:CC:DD:EE:FF")
     *
     * Returns the JSON string containing ThpCredentials, or None if not found.
     */
    public fun `loadThpCredential`(`deviceId`: kotlin.String): kotlin.String?
    
    /**
     * Log a debug message from the Rust THP handshake layer.
     *
     * This forwards Rust-level errors and state information to the native
     * debug UI (e.g., TrezorDebugLog on Android) so they are visible
     * alongside the Kotlin-level logs.
     *
     * # Arguments
     * * `tag` - Short tag identifying the subsystem (e.g., "HANDSHAKE", "THP")
     * * `message` - Human-readable debug message
     */
    public fun `logDebug`(`tag`: kotlin.String, `message`: kotlin.String)
    
    public companion object
}




/**
 * Callback interface for handling PIN and passphrase requests from the Trezor device.
 *
 * The native layer (iOS/Android) should implement this to show PIN/passphrase
 * input UI when the device requests it during operations like signing.
 *
 * Methods return `String`:
 * - Empty string (`""`) = cancel the request
 * - Non-empty string = the user's input (PIN or passphrase)
 *
 * This matches the existing `get_pairing_code` pattern used in `TrezorTransportCallback`.
 */
public interface TrezorUiCallback {
    
    /**
     * Called when the device requests a PIN.
     *
     * Show a PIN matrix UI and return the matrix-encoded PIN string.
     * Return empty string to cancel.
     */
    public fun `onPinRequest`(): kotlin.String
    
    /**
     * Called when the device requests a passphrase.
     *
     * If `on_device` is true, the user should enter on the Trezor itself —
     * return any non-empty string (e.g., "ok") to acknowledge.
     *
     * If `on_device` is false, show a passphrase input UI and return the value.
     * Return empty string to cancel.
     */
    public fun `onPassphraseRequest`(`onDevice`: kotlin.Boolean): kotlin.String
    
    public companion object
}




/**
 * Account addresses
 */
@kotlinx.serialization.Serializable
public data class AccountAddresses (
    /**
     * Used addresses
     */
    val `used`: List<AddressInfo>, 
    /**
     * Unused addresses
     */
    val `unused`: List<AddressInfo>, 
    /**
     * Change addresses
     */
    val `change`: List<AddressInfo>
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class ActivityTags (
    val `activityId`: kotlin.String, 
    val `tags`: List<kotlin.String>
) {
    public companion object
}



/**
 * Address information
 */
@kotlinx.serialization.Serializable
public data class AddressInfo (
    /**
     * Address string
     */
    val `address`: kotlin.String, 
    /**
     * Derivation path
     */
    val `path`: kotlin.String, 
    /**
     * Number of transfers
     */
    val `transfers`: kotlin.UInt
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class ChannelLiquidityOptions (
    val `defaultLspBalanceSat`: kotlin.ULong, 
    val `minLspBalanceSat`: kotlin.ULong, 
    val `maxLspBalanceSat`: kotlin.ULong, 
    val `maxClientBalanceSat`: kotlin.ULong
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class ChannelLiquidityParams (
    val `clientBalanceSat`: kotlin.ULong, 
    val `existingChannelsTotalSat`: kotlin.ULong, 
    val `minChannelSizeSat`: kotlin.ULong, 
    val `maxChannelSizeSat`: kotlin.ULong, 
    val `satsPerEur`: kotlin.ULong
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class ClosedChannelDetails (
    val `channelId`: kotlin.String, 
    val `counterpartyNodeId`: kotlin.String, 
    val `fundingTxoTxid`: kotlin.String, 
    val `fundingTxoIndex`: kotlin.UInt, 
    val `channelValueSats`: kotlin.ULong, 
    val `closedAt`: kotlin.ULong, 
    val `outboundCapacityMsat`: kotlin.ULong, 
    val `inboundCapacityMsat`: kotlin.ULong, 
    val `counterpartyUnspendablePunishmentReserve`: kotlin.ULong, 
    val `unspendablePunishmentReserve`: kotlin.ULong, 
    val `forwardingFeeProportionalMillionths`: kotlin.UInt, 
    val `forwardingFeeBaseMsat`: kotlin.UInt, 
    val `channelName`: kotlin.String, 
    val `channelClosureReason`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class CreateCjitOptions (
    val `source`: kotlin.String?, 
    val `discountCode`: kotlin.String?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class CreateOrderOptions (
    val `clientBalanceSat`: kotlin.ULong, 
    val `lspNodeId`: kotlin.String?, 
    val `couponCode`: kotlin.String, 
    val `source`: kotlin.String?, 
    val `discountCode`: kotlin.String?, 
    val `zeroConf`: kotlin.Boolean, 
    val `zeroConfPayment`: kotlin.Boolean?, 
    val `zeroReserve`: kotlin.Boolean, 
    val `clientNodeId`: kotlin.String?, 
    val `signature`: kotlin.String?, 
    val `timestamp`: kotlin.String?, 
    val `refundOnchainAddress`: kotlin.String?, 
    val `announceChannel`: kotlin.Boolean
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class DefaultLspBalanceParams (
    val `clientBalanceSat`: kotlin.ULong, 
    val `maxChannelSizeSat`: kotlin.ULong, 
    val `satsPerEur`: kotlin.ULong
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class ErrorData (
    val `errorDetails`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class FeeRates (
    val `fast`: kotlin.UInt, 
    val `mid`: kotlin.UInt, 
    val `slow`: kotlin.UInt
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class FundingTx (
    val `id`: kotlin.String, 
    val `vout`: kotlin.ULong
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class GetAddressResponse (
    /**
     * The generated Bitcoin address as a string
     */
    val `address`: kotlin.String, 
    /**
     * The derivation path used to generate the address
     */
    val `path`: kotlin.String, 
    /**
     * The hexadecimal representation of the public key
     */
    val `publicKey`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class GetAddressesResponse (
    /**
     * Vector of generated Bitcoin addresses
     */
    val `addresses`: List<GetAddressResponse>
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IBt0ConfMinTxFeeWindow (
    val `satPerVbyte`: kotlin.Double, 
    val `validityEndsAt`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IBtBolt11Invoice (
    val `request`: kotlin.String, 
    val `state`: BtBolt11InvoiceState, 
    val `expiresAt`: kotlin.String, 
    val `updatedAt`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IBtChannel (
    val `state`: BtOpenChannelState, 
    val `lspNodePubkey`: kotlin.String, 
    val `clientNodePubkey`: kotlin.String, 
    val `announceChannel`: kotlin.Boolean, 
    val `fundingTx`: FundingTx, 
    val `closingTxId`: kotlin.String?, 
    val `close`: IBtChannelClose?, 
    val `shortChannelId`: kotlin.String?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IBtChannelClose (
    val `txId`: kotlin.String, 
    val `closeType`: kotlin.String, 
    val `initiator`: kotlin.String, 
    val `registeredAt`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IBtEstimateFeeResponse (
    val `feeSat`: kotlin.ULong, 
    val `min0ConfTxFee`: IBt0ConfMinTxFeeWindow
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IBtEstimateFeeResponse2 (
    val `feeSat`: kotlin.ULong, 
    val `networkFeeSat`: kotlin.ULong, 
    val `serviceFeeSat`: kotlin.ULong, 
    val `min0ConfTxFee`: IBt0ConfMinTxFeeWindow
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IBtInfo (
    val `version`: kotlin.UInt, 
    val `nodes`: List<ILspNode>, 
    val `options`: IBtInfoOptions, 
    val `versions`: IBtInfoVersions, 
    val `onchain`: IBtInfoOnchain
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IBtInfoOnchain (
    val `network`: BitcoinNetworkEnum, 
    val `feeRates`: FeeRates
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IBtInfoOptions (
    val `minChannelSizeSat`: kotlin.ULong, 
    val `maxChannelSizeSat`: kotlin.ULong, 
    val `minExpiryWeeks`: kotlin.UInt, 
    val `maxExpiryWeeks`: kotlin.UInt, 
    val `minPaymentConfirmations`: kotlin.UInt, 
    val `minHighRiskPaymentConfirmations`: kotlin.UInt, 
    val `max0ConfClientBalanceSat`: kotlin.ULong, 
    val `maxClientBalanceSat`: kotlin.ULong
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IBtInfoVersions (
    val `http`: kotlin.String, 
    val `btc`: kotlin.String, 
    val `ln2`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IBtOnchainTransaction (
    val `amountSat`: kotlin.ULong, 
    val `txId`: kotlin.String, 
    val `vout`: kotlin.UInt, 
    val `blockHeight`: kotlin.UInt?, 
    val `blockConfirmationCount`: kotlin.UInt, 
    val `feeRateSatPerVbyte`: kotlin.Double, 
    val `confirmed`: kotlin.Boolean, 
    val `suspicious0ConfReason`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IBtOnchainTransactions (
    val `address`: kotlin.String, 
    val `confirmedSat`: kotlin.ULong, 
    val `requiredConfirmations`: kotlin.UInt, 
    val `transactions`: List<IBtOnchainTransaction>
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IBtOrder (
    val `id`: kotlin.String, 
    val `state`: BtOrderState, 
    val `state2`: BtOrderState2?, 
    val `feeSat`: kotlin.ULong, 
    val `networkFeeSat`: kotlin.ULong, 
    val `serviceFeeSat`: kotlin.ULong, 
    val `lspBalanceSat`: kotlin.ULong, 
    val `clientBalanceSat`: kotlin.ULong, 
    val `zeroConf`: kotlin.Boolean, 
    val `zeroReserve`: kotlin.Boolean, 
    val `clientNodeId`: kotlin.String?, 
    val `channelExpiryWeeks`: kotlin.UInt, 
    val `channelExpiresAt`: kotlin.String, 
    val `orderExpiresAt`: kotlin.String, 
    val `channel`: IBtChannel?, 
    val `lspNode`: ILspNode?, 
    val `lnurl`: kotlin.String?, 
    val `payment`: IBtPayment?, 
    val `couponCode`: kotlin.String?, 
    val `source`: kotlin.String?, 
    val `discount`: IDiscount?, 
    val `updatedAt`: kotlin.String, 
    val `createdAt`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IBtPayment (
    val `state`: BtPaymentState, 
    val `state2`: BtPaymentState2?, 
    val `paidSat`: kotlin.ULong, 
    val `bolt11Invoice`: IBtBolt11Invoice?, 
    val `onchain`: IBtOnchainTransactions?, 
    val `isManuallyPaid`: kotlin.Boolean?, 
    val `manualRefunds`: List<IManualRefund>?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IcJitEntry (
    val `id`: kotlin.String, 
    val `state`: CJitStateEnum, 
    val `feeSat`: kotlin.ULong, 
    val `networkFeeSat`: kotlin.ULong, 
    val `serviceFeeSat`: kotlin.ULong, 
    val `channelSizeSat`: kotlin.ULong, 
    val `channelExpiryWeeks`: kotlin.UInt, 
    val `channelOpenError`: kotlin.String?, 
    val `nodeId`: kotlin.String, 
    val `invoice`: IBtBolt11Invoice, 
    val `channel`: IBtChannel?, 
    val `lspNode`: ILspNode, 
    val `couponCode`: kotlin.String, 
    val `source`: kotlin.String?, 
    val `discount`: IDiscount?, 
    val `expiresAt`: kotlin.String, 
    val `updatedAt`: kotlin.String, 
    val `createdAt`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IDiscount (
    val `code`: kotlin.String, 
    val `absoluteSat`: kotlin.ULong, 
    val `relative`: kotlin.Double, 
    val `overallSat`: kotlin.ULong
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IGift (
    val `id`: kotlin.String, 
    val `nodeId`: kotlin.String, 
    val `orderId`: kotlin.String?, 
    val `order`: IGiftOrder?, 
    val `bolt11PaymentId`: kotlin.String?, 
    val `bolt11Payment`: IGiftPayment?, 
    val `appliedGiftCodeId`: kotlin.String?, 
    val `appliedGiftCode`: IGiftCode?, 
    val `createdAt`: kotlin.String?, 
    val `updatedAt`: kotlin.String?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IGiftBolt11Invoice (
    val `id`: kotlin.String, 
    val `request`: kotlin.String, 
    val `state`: kotlin.String, 
    val `isHodlInvoice`: kotlin.Boolean?, 
    val `paymentHash`: kotlin.String?, 
    val `amountSat`: kotlin.ULong?, 
    val `amountMsat`: kotlin.String?, 
    val `internalNodePubkey`: kotlin.String?, 
    val `updatedAt`: kotlin.String?, 
    val `createdAt`: kotlin.String?, 
    val `expiresAt`: kotlin.String?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IGiftBtcAddress (
    val `id`: kotlin.String, 
    val `address`: kotlin.String, 
    val `transactions`: List<kotlin.String>, 
    val `allTransactions`: List<kotlin.String>, 
    val `isBlacklisted`: kotlin.Boolean?, 
    val `watchUntil`: kotlin.String?, 
    val `watchForBlockConfirmations`: kotlin.UInt?, 
    val `updatedAt`: kotlin.String?, 
    val `createdAt`: kotlin.String?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IGiftCode (
    val `id`: kotlin.String, 
    val `code`: kotlin.String, 
    val `createdAt`: kotlin.String, 
    val `updatedAt`: kotlin.String, 
    val `expiresAt`: kotlin.String, 
    val `giftSat`: kotlin.ULong?, 
    val `scope`: kotlin.String?, 
    val `maxCount`: kotlin.UInt?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IGiftLspNode (
    val `alias`: kotlin.String, 
    val `pubkey`: kotlin.String, 
    val `connectionStrings`: List<kotlin.String>
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IGiftOrder (
    val `id`: kotlin.String, 
    val `state`: kotlin.String, 
    val `oldState`: kotlin.String?, 
    val `isChannelExpired`: kotlin.Boolean?, 
    val `isOrderExpired`: kotlin.Boolean?, 
    val `lspBalanceSat`: kotlin.ULong?, 
    val `clientBalanceSat`: kotlin.ULong?, 
    val `channelExpiryWeeks`: kotlin.UInt?, 
    val `zeroConf`: kotlin.Boolean?, 
    val `zeroReserve`: kotlin.Boolean?, 
    val `announced`: kotlin.Boolean?, 
    val `clientNodeId`: kotlin.String?, 
    val `channelExpiresAt`: kotlin.String?, 
    val `orderExpiresAt`: kotlin.String?, 
    val `feeSat`: kotlin.ULong?, 
    val `networkFeeSat`: kotlin.ULong?, 
    val `serviceFeeSat`: kotlin.ULong?, 
    val `payment`: IGiftPayment?, 
    val `lspNode`: IGiftLspNode?, 
    val `updatedAt`: kotlin.String?, 
    val `createdAt`: kotlin.String?, 
    val `nodeIdVerified`: kotlin.Boolean?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IGiftPayment (
    val `id`: kotlin.String, 
    val `state`: kotlin.String, 
    val `oldState`: kotlin.String?, 
    val `onchainState`: kotlin.String?, 
    val `lnState`: kotlin.String?, 
    val `paidOnchainSat`: kotlin.ULong?, 
    val `paidLnSat`: kotlin.ULong?, 
    val `paidSat`: kotlin.ULong?, 
    val `isOverpaid`: kotlin.Boolean?, 
    val `isRefunded`: kotlin.Boolean?, 
    val `overpaidAmountSat`: kotlin.ULong?, 
    val `requiredOnchainConfirmations`: kotlin.UInt?, 
    val `settlementState`: kotlin.String?, 
    val `expectedAmountSat`: kotlin.ULong?, 
    val `isManuallyPaid`: kotlin.Boolean?, 
    val `btcAddress`: IGiftBtcAddress?, 
    val `btcAddressId`: kotlin.String?, 
    val `bolt11Invoice`: IGiftBolt11Invoice?, 
    val `bolt11InvoiceId`: kotlin.String?, 
    val `manualRefunds`: List<kotlin.String>
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class ILspNode (
    val `alias`: kotlin.String, 
    val `pubkey`: kotlin.String, 
    val `connectionStrings`: List<kotlin.String>, 
    val `readonly`: kotlin.Boolean?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class IManualRefund (
    val `amountSat`: kotlin.ULong, 
    val `target`: kotlin.String, 
    val `state`: ManualRefundStateEnum, 
    val `createdByName`: kotlin.String, 
    val `votedByName`: kotlin.String?, 
    val `reason`: kotlin.String?, 
    val `targetType`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class LightningActivity (
    val `id`: kotlin.String, 
    val `txType`: PaymentType, 
    val `status`: PaymentState, 
    val `value`: kotlin.ULong, 
    val `fee`: kotlin.ULong?, 
    val `invoice`: kotlin.String, 
    val `message`: kotlin.String, 
    val `timestamp`: kotlin.ULong, 
    val `preimage`: kotlin.String?, 
    val `createdAt`: kotlin.ULong?, 
    val `updatedAt`: kotlin.ULong?, 
    val `seenAt`: kotlin.ULong?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class LightningInvoice (
    val `bolt11`: kotlin.String, 
    val `paymentHash`: kotlin.ByteArray, 
    val `amountSatoshis`: kotlin.ULong, 
    val `timestampSeconds`: kotlin.ULong, 
    val `expirySeconds`: kotlin.ULong, 
    val `isExpired`: kotlin.Boolean, 
    val `description`: kotlin.String?, 
    val `networkType`: NetworkType, 
    val `payeeNodeId`: kotlin.ByteArray?
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other == null || this::class != other::class) return false

        other as LightningInvoice
        if (`bolt11` != other.`bolt11`) return false
        if (!`paymentHash`.contentEquals(other.`paymentHash`)) return false
        if (`amountSatoshis` != other.`amountSatoshis`) return false
        if (`timestampSeconds` != other.`timestampSeconds`) return false
        if (`expirySeconds` != other.`expirySeconds`) return false
        if (`isExpired` != other.`isExpired`) return false
        if (`description` != other.`description`) return false
        if (`networkType` != other.`networkType`) return false
        if (`payeeNodeId` != null) {
            if (other.`payeeNodeId` == null) return false
            if (!`payeeNodeId`.contentEquals(other.`payeeNodeId`)) return false
        }

        return true
    }
    override fun hashCode(): Int {
        var result = `bolt11`.hashCode()
        result = 31 * result + `paymentHash`.contentHashCode()
        result = 31 * result + `amountSatoshis`.hashCode()
        result = 31 * result + `timestampSeconds`.hashCode()
        result = 31 * result + `expirySeconds`.hashCode()
        result = 31 * result + `isExpired`.hashCode()
        result = 31 * result + (`description`?.hashCode() ?: 0)
        result = 31 * result + `networkType`.hashCode()
        result = 31 * result + (`payeeNodeId`?.contentHashCode() ?: 0)
        return result
    }
    public companion object
}



@kotlinx.serialization.Serializable
public data class LnurlAddressData (
    val `uri`: kotlin.String, 
    val `domain`: kotlin.String, 
    val `username`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class LnurlAuthData (
    val `uri`: kotlin.String, 
    val `tag`: kotlin.String, 
    val `k1`: kotlin.String, 
    val `domain`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class LnurlChannelData (
    val `uri`: kotlin.String, 
    val `callback`: kotlin.String, 
    val `k1`: kotlin.String, 
    val `tag`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class LnurlPayData (
    val `uri`: kotlin.String, 
    val `callback`: kotlin.String, 
    val `minSendable`: kotlin.ULong, 
    val `maxSendable`: kotlin.ULong, 
    val `metadataStr`: kotlin.String, 
    val `commentAllowed`: kotlin.UInt?, 
    val `allowsNostr`: kotlin.Boolean, 
    val `nostrPubkey`: kotlin.ByteArray?
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other == null || this::class != other::class) return false

        other as LnurlPayData
        if (`uri` != other.`uri`) return false
        if (`callback` != other.`callback`) return false
        if (`minSendable` != other.`minSendable`) return false
        if (`maxSendable` != other.`maxSendable`) return false
        if (`metadataStr` != other.`metadataStr`) return false
        if (`commentAllowed` != other.`commentAllowed`) return false
        if (`allowsNostr` != other.`allowsNostr`) return false
        if (`nostrPubkey` != null) {
            if (other.`nostrPubkey` == null) return false
            if (!`nostrPubkey`.contentEquals(other.`nostrPubkey`)) return false
        }

        return true
    }
    override fun hashCode(): Int {
        var result = `uri`.hashCode()
        result = 31 * result + `callback`.hashCode()
        result = 31 * result + `minSendable`.hashCode()
        result = 31 * result + `maxSendable`.hashCode()
        result = 31 * result + `metadataStr`.hashCode()
        result = 31 * result + (`commentAllowed`?.hashCode() ?: 0)
        result = 31 * result + `allowsNostr`.hashCode()
        result = 31 * result + (`nostrPubkey`?.contentHashCode() ?: 0)
        return result
    }
    public companion object
}



@kotlinx.serialization.Serializable
public data class LnurlWithdrawData (
    val `uri`: kotlin.String, 
    val `callback`: kotlin.String, 
    val `k1`: kotlin.String, 
    val `defaultDescription`: kotlin.String, 
    val `minWithdrawable`: kotlin.ULong?, 
    val `maxWithdrawable`: kotlin.ULong, 
    val `tag`: kotlin.String
) {
    public companion object
}



/**
 * Native device information returned from enumeration
 */
@kotlinx.serialization.Serializable
public data class NativeDeviceInfo (
    /**
     * Unique path/identifier for this device
     */
    val `path`: kotlin.String, 
    /**
     * Transport type: "usb" or "bluetooth"
     */
    val `transportType`: kotlin.String, 
    /**
     * Optional device name (from BLE advertisement or USB descriptor)
     */
    val `name`: kotlin.String?, 
    /**
     * USB Vendor ID (for USB devices)
     */
    val `vendorId`: kotlin.UShort?, 
    /**
     * USB Product ID (for USB devices)
     */
    val `productId`: kotlin.UShort?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class OnChainInvoice (
    val `address`: kotlin.String, 
    val `amountSatoshis`: kotlin.ULong, 
    val `label`: kotlin.String?, 
    val `message`: kotlin.String?, 
    val `params`: Map<kotlin.String, kotlin.String>?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class OnchainActivity (
    val `id`: kotlin.String, 
    val `txType`: PaymentType, 
    val `txId`: kotlin.String, 
    val `value`: kotlin.ULong, 
    val `fee`: kotlin.ULong, 
    val `feeRate`: kotlin.ULong, 
    val `address`: kotlin.String, 
    val `confirmed`: kotlin.Boolean, 
    val `timestamp`: kotlin.ULong, 
    val `isBoosted`: kotlin.Boolean, 
    val `boostTxIds`: List<kotlin.String>, 
    val `isTransfer`: kotlin.Boolean, 
    val `doesExist`: kotlin.Boolean, 
    val `confirmTimestamp`: kotlin.ULong?, 
    val `channelId`: kotlin.String?, 
    val `transferTxId`: kotlin.String?, 
    val `createdAt`: kotlin.ULong?, 
    val `updatedAt`: kotlin.ULong?, 
    val `seenAt`: kotlin.ULong?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class PreActivityMetadata (
    val `paymentId`: kotlin.String, 
    val `tags`: List<kotlin.String>, 
    val `paymentHash`: kotlin.String?, 
    val `txId`: kotlin.String?, 
    val `address`: kotlin.String?, 
    val `isReceive`: kotlin.Boolean, 
    val `feeRate`: kotlin.ULong, 
    val `isTransfer`: kotlin.Boolean, 
    val `channelId`: kotlin.String?, 
    val `createdAt`: kotlin.ULong
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class PubkyAuth (
    val `data`: kotlin.String
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class SweepResult (
    /**
     * The transaction ID of the sweep transaction
     */
    val `txid`: kotlin.String, 
    /**
     * The total amount swept (in satoshis)
     */
    val `amountSwept`: kotlin.ULong, 
    /**
     * The fee paid (in satoshis)
     */
    val `feePaid`: kotlin.ULong, 
    /**
     * The number of UTXOs swept
     */
    val `utxosSwept`: kotlin.UInt
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class SweepTransactionPreview (
    /**
     * The PSBT (Partially Signed Bitcoin Transaction) in base64 format
     */
    val `psbt`: kotlin.String, 
    /**
     * The total amount available to sweep (in satoshis)
     */
    val `totalAmount`: kotlin.ULong, 
    /**
     * The estimated fee for the transaction (in satoshis)
     */
    val `estimatedFee`: kotlin.ULong, 
    /**
     * The estimated virtual size of the transaction (in vbytes)
     */
    val `estimatedVsize`: kotlin.ULong, 
    /**
     * The number of UTXOs that will be swept
     */
    val `utxosCount`: kotlin.UInt, 
    /**
     * The destination address
     */
    val `destinationAddress`: kotlin.String, 
    /**
     * The amount that will be sent to destination after fees (in satoshis)
     */
    val `amountAfterFees`: kotlin.ULong
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class SweepableBalances (
    /**
     * Balance in legacy (P2PKH) addresses (in satoshis)
     */
    val `legacyBalance`: kotlin.ULong, 
    /**
     * Balance in P2SH-SegWit (P2SH-P2WPKH) addresses (in satoshis)
     */
    val `p2shBalance`: kotlin.ULong, 
    /**
     * Balance in Taproot (P2TR) addresses (in satoshis)
     */
    val `taprootBalance`: kotlin.ULong, 
    /**
     * Total balance across all wallet types (in satoshis)
     */
    val `totalBalance`: kotlin.ULong, 
    /**
     * Number of UTXOs in legacy wallet
     */
    val `legacyUtxosCount`: kotlin.UInt, 
    /**
     * Number of UTXOs in P2SH-SegWit wallet
     */
    val `p2shUtxosCount`: kotlin.UInt, 
    /**
     * Number of UTXOs in Taproot wallet
     */
    val `taprootUtxosCount`: kotlin.UInt, 
    /**
     * Total number of UTXOs across all wallet types
     */
    val `totalUtxosCount`: kotlin.UInt
) {
    public companion object
}



/**
 * Details about an onchain transaction.
 */
@kotlinx.serialization.Serializable
public data class TransactionDetails (
    /**
     * The transaction ID.
     */
    val `txId`: kotlin.String, 
    /**
     * The net amount in this transaction (in satoshis).
     *
     * This is calculated as: (received - sent). For incoming payments,
     * this will be positive. For outgoing payments, this will be negative.
     *
     * Note: This amount does NOT include transaction fees.
     */
    val `amountSats`: kotlin.Long, 
    /**
     * The transaction inputs with full details.
     */
    val `inputs`: List<TxInput>, 
    /**
     * The transaction outputs with full details.
     */
    val `outputs`: List<TxOutput>
) {
    public companion object
}



/**
 * Address response from device.
 */
@kotlinx.serialization.Serializable
public data class TrezorAddressResponse (
    /**
     * The Bitcoin address
     */
    val `address`: kotlin.String, 
    /**
     * The serialized path (e.g., "m/84'/0'/0'/0/0")
     */
    val `path`: kotlin.String
) {
    public companion object
}



/**
 * Result from a high-level message call (for BLE/THP devices)
 */
@kotlinx.serialization.Serializable
public data class TrezorCallMessageResult (
    /**
     * Whether the call succeeded
     */
    val `success`: kotlin.Boolean, 
    /**
     * Response message type
     */
    val `messageType`: kotlin.UShort, 
    /**
     * Response protobuf data
     */
    val `data`: kotlin.ByteArray, 
    /**
     * Error message (empty on success)
     */
    val `error`: kotlin.String
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other == null || this::class != other::class) return false

        other as TrezorCallMessageResult
        if (`success` != other.`success`) return false
        if (`messageType` != other.`messageType`) return false
        if (!`data`.contentEquals(other.`data`)) return false
        if (`error` != other.`error`) return false

        return true
    }
    override fun hashCode(): Int {
        var result = `success`.hashCode()
        result = 31 * result + `messageType`.hashCode()
        result = 31 * result + `data`.contentHashCode()
        result = 31 * result + `error`.hashCode()
        return result
    }
    public companion object
}



/**
 * Device information exposed to FFI.
 */
@kotlinx.serialization.Serializable
public data class TrezorDeviceInfo (
    /**
     * Unique identifier for the device
     */
    val `id`: kotlin.String, 
    /**
     * Transport type (USB or Bluetooth)
     */
    val `transportType`: TrezorTransportType, 
    /**
     * Device name (from BLE advertisement or USB descriptor)
     */
    val `name`: kotlin.String?, 
    /**
     * Transport-specific path (used internally for connection)
     */
    val `path`: kotlin.String, 
    /**
     * Device label (set by user during device setup)
     */
    val `label`: kotlin.String?, 
    /**
     * Device model (e.g., "T2", "Safe 5", "Safe 7")
     */
    val `model`: kotlin.String?, 
    /**
     * Whether the device is in bootloader mode
     */
    val `isBootloader`: kotlin.Boolean
) {
    public companion object
}



/**
 * Device features after initialization.
 */
@kotlinx.serialization.Serializable
public data class TrezorFeatures (
    /**
     * Vendor string
     */
    val `vendor`: kotlin.String?, 
    /**
     * Device model
     */
    val `model`: kotlin.String?, 
    /**
     * Device label (set by user during device setup)
     */
    val `label`: kotlin.String?, 
    /**
     * Device ID (unique per device)
     */
    val `deviceId`: kotlin.String?, 
    /**
     * Major firmware version
     */
    val `majorVersion`: kotlin.UInt?, 
    /**
     * Minor firmware version
     */
    val `minorVersion`: kotlin.UInt?, 
    /**
     * Patch firmware version
     */
    val `patchVersion`: kotlin.UInt?, 
    /**
     * Whether PIN protection is enabled
     */
    val `pinProtection`: kotlin.Boolean?, 
    /**
     * Whether passphrase protection is enabled
     */
    val `passphraseProtection`: kotlin.Boolean?, 
    /**
     * Whether the device is initialized with a seed
     */
    val `initialized`: kotlin.Boolean?, 
    /**
     * Whether the device needs backup
     */
    val `needsBackup`: kotlin.Boolean?
) {
    public companion object
}



/**
 * Parameters for getting an address from the device.
 */
@kotlinx.serialization.Serializable
public data class TrezorGetAddressParams (
    /**
     * BIP32 path (e.g., "m/84'/0'/0'/0/0")
     */
    val `path`: kotlin.String, 
    /**
     * Coin network (default: Bitcoin)
     */
    val `coin`: TrezorCoinType?, 
    /**
     * Whether to display the address on the device for confirmation
     */
    val `showOnTrezor`: kotlin.Boolean, 
    /**
     * Script type (auto-detected from path if not specified)
     */
    val `scriptType`: TrezorScriptType?
) {
    public companion object
}



/**
 * Parameters for getting a public key from the device.
 */
@kotlinx.serialization.Serializable
public data class TrezorGetPublicKeyParams (
    /**
     * BIP32 path (e.g., "m/84'/0'/0'")
     */
    val `path`: kotlin.String, 
    /**
     * Coin network (default: Bitcoin)
     */
    val `coin`: TrezorCoinType?, 
    /**
     * Whether to display on device for confirmation
     */
    val `showOnTrezor`: kotlin.Boolean
) {
    public companion object
}



/**
 * Previous transaction data (for non-SegWit input verification).
 */
@kotlinx.serialization.Serializable
public data class TrezorPrevTx (
    /**
     * Transaction hash (hex encoded)
     */
    val `hash`: kotlin.String, 
    /**
     * Transaction version
     */
    val `version`: kotlin.UInt, 
    /**
     * Lock time
     */
    val `lockTime`: kotlin.UInt, 
    /**
     * Transaction inputs
     */
    val `inputs`: List<TrezorPrevTxInput>, 
    /**
     * Transaction outputs
     */
    val `outputs`: List<TrezorPrevTxOutput>
) {
    public companion object
}



/**
 * Input of a previous transaction.
 */
@kotlinx.serialization.Serializable
public data class TrezorPrevTxInput (
    /**
     * Previous transaction hash (hex encoded)
     */
    val `prevHash`: kotlin.String, 
    /**
     * Previous output index
     */
    val `prevIndex`: kotlin.UInt, 
    /**
     * Script signature (hex encoded)
     */
    val `scriptSig`: kotlin.String, 
    /**
     * Sequence number
     */
    val `sequence`: kotlin.UInt
) {
    public companion object
}



/**
 * Output of a previous transaction.
 */
@kotlinx.serialization.Serializable
public data class TrezorPrevTxOutput (
    /**
     * Amount in satoshis
     */
    val `amount`: kotlin.ULong, 
    /**
     * Script pubkey (hex encoded)
     */
    val `scriptPubkey`: kotlin.String
) {
    public companion object
}



/**
 * Public key response from device.
 */
@kotlinx.serialization.Serializable
public data class TrezorPublicKeyResponse (
    /**
     * Extended public key (xpub)
     */
    val `xpub`: kotlin.String, 
    /**
     * The serialized path (e.g., "m/84'/0'/0'")
     */
    val `path`: kotlin.String, 
    /**
     * Compressed public key (hex encoded)
     */
    val `publicKey`: kotlin.String, 
    /**
     * Chain code (hex encoded)
     */
    val `chainCode`: kotlin.String, 
    /**
     * Parent key fingerprint
     */
    val `fingerprint`: kotlin.UInt, 
    /**
     * Derivation depth
     */
    val `depth`: kotlin.UInt, 
    /**
     * Master root fingerprint (from the device's master seed)
     */
    val `rootFingerprint`: kotlin.UInt?
) {
    public companion object
}



/**
 * Parameters for signing a message.
 */
@kotlinx.serialization.Serializable
public data class TrezorSignMessageParams (
    /**
     * BIP32 path for the signing key (e.g., "m/84'/0'/0'/0/0")
     */
    val `path`: kotlin.String, 
    /**
     * Message to sign
     */
    val `message`: kotlin.String, 
    /**
     * Coin network (default: Bitcoin)
     */
    val `coin`: TrezorCoinType?
) {
    public companion object
}



/**
 * Parameters for signing a transaction.
 */
@kotlinx.serialization.Serializable
public data class TrezorSignTxParams (
    /**
     * Transaction inputs
     */
    val `inputs`: List<TrezorTxInput>, 
    /**
     * Transaction outputs
     */
    val `outputs`: List<TrezorTxOutput>, 
    /**
     * Coin network (default: Bitcoin)
     */
    val `coin`: TrezorCoinType?, 
    /**
     * Lock time (default: 0)
     */
    val `lockTime`: kotlin.UInt?, 
    /**
     * Version (default: 2)
     */
    val `version`: kotlin.UInt?, 
    /**
     * Previous transactions (for non-SegWit input verification)
     */
    val `prevTxs`: List<TrezorPrevTx>
) {
    public companion object
}



/**
 * Response from signing a message.
 */
@kotlinx.serialization.Serializable
public data class TrezorSignedMessageResponse (
    /**
     * Bitcoin address that signed the message
     */
    val `address`: kotlin.String, 
    /**
     * Signature (base64 encoded)
     */
    val `signature`: kotlin.String
) {
    public companion object
}



/**
 * Signed transaction result.
 */
@kotlinx.serialization.Serializable
public data class TrezorSignedTx (
    /**
     * Signatures for each input (hex encoded)
     */
    val `signatures`: List<kotlin.String>, 
    /**
     * Serialized transaction (hex)
     */
    val `serializedTx`: kotlin.String
) {
    public companion object
}



/**
 * Result from a transport read operation
 */
@kotlinx.serialization.Serializable
public data class TrezorTransportReadResult (
    /**
     * Whether the read succeeded
     */
    val `success`: kotlin.Boolean, 
    /**
     * Data read (empty on failure)
     */
    val `data`: kotlin.ByteArray, 
    /**
     * Error message (empty on success)
     */
    val `error`: kotlin.String
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other == null || this::class != other::class) return false

        other as TrezorTransportReadResult
        if (`success` != other.`success`) return false
        if (!`data`.contentEquals(other.`data`)) return false
        if (`error` != other.`error`) return false

        return true
    }
    override fun hashCode(): Int {
        var result = `success`.hashCode()
        result = 31 * result + `data`.contentHashCode()
        result = 31 * result + `error`.hashCode()
        return result
    }
    public companion object
}



/**
 * Result from a transport write or open operation
 */
@kotlinx.serialization.Serializable
public data class TrezorTransportWriteResult (
    /**
     * Whether the operation succeeded
     */
    val `success`: kotlin.Boolean, 
    /**
     * Error message (empty on success)
     */
    val `error`: kotlin.String
) {
    public companion object
}



/**
 * Transaction input for signing.
 */
@kotlinx.serialization.Serializable
public data class TrezorTxInput (
    /**
     * Previous transaction hash (hex, 32 bytes)
     */
    val `prevHash`: kotlin.String, 
    /**
     * Previous output index
     */
    val `prevIndex`: kotlin.UInt, 
    /**
     * BIP32 derivation path (e.g., "m/84'/0'/0'/0/0")
     */
    val `path`: kotlin.String, 
    /**
     * Amount in satoshis
     */
    val `amount`: kotlin.ULong, 
    /**
     * Script type
     */
    val `scriptType`: TrezorScriptType, 
    /**
     * Sequence number (default: 0xFFFFFFFD for RBF)
     */
    val `sequence`: kotlin.UInt?, 
    /**
     * Original transaction hash for RBF replacement (hex encoded)
     */
    val `origHash`: kotlin.String?, 
    /**
     * Original input index for RBF replacement
     */
    val `origIndex`: kotlin.UInt?
) {
    public companion object
}



/**
 * Transaction output for signing.
 */
@kotlinx.serialization.Serializable
public data class TrezorTxOutput (
    /**
     * Destination address (for external outputs)
     */
    val `address`: kotlin.String?, 
    /**
     * BIP32 path (for change outputs)
     */
    val `path`: kotlin.String?, 
    /**
     * Amount in satoshis
     */
    val `amount`: kotlin.ULong, 
    /**
     * Script type (for change outputs)
     */
    val `scriptType`: TrezorScriptType?, 
    /**
     * OP_RETURN data (hex encoded, for data outputs)
     */
    val `opReturnData`: kotlin.String?, 
    /**
     * Original transaction hash for RBF replacement (hex encoded)
     */
    val `origHash`: kotlin.String?, 
    /**
     * Original output index for RBF replacement
     */
    val `origIndex`: kotlin.UInt?
) {
    public companion object
}



/**
 * Parameters for verifying a message signature.
 */
@kotlinx.serialization.Serializable
public data class TrezorVerifyMessageParams (
    /**
     * Bitcoin address that signed the message
     */
    val `address`: kotlin.String, 
    /**
     * Signature (base64 encoded)
     */
    val `signature`: kotlin.String, 
    /**
     * Original message
     */
    val `message`: kotlin.String, 
    /**
     * Coin network (default: Bitcoin)
     */
    val `coin`: TrezorCoinType?
) {
    public companion object
}



/**
 * Details about a transaction input.
 */
@kotlinx.serialization.Serializable
public data class TxInput (
    /**
     * The transaction ID of the previous output being spent.
     */
    val `txid`: kotlin.String, 
    /**
     * The output index of the previous output being spent.
     */
    val `vout`: kotlin.UInt, 
    /**
     * The script signature (hex-encoded).
     */
    val `scriptsig`: kotlin.String, 
    /**
     * The witness stack (hex-encoded strings).
     */
    val `witness`: List<kotlin.String>, 
    /**
     * The sequence number.
     */
    val `sequence`: kotlin.UInt
) {
    public companion object
}



/**
 * Details about a transaction output.
 */
@kotlinx.serialization.Serializable
public data class TxOutput (
    /**
     * The script public key (hex-encoded).
     */
    val `scriptpubkey`: kotlin.String, 
    /**
     * The script public key type (e.g., "p2pkh", "p2sh", "p2wpkh", "p2wsh", "p2tr").
     */
    val `scriptpubkeyType`: kotlin.String?, 
    /**
     * The address corresponding to this script (if decodable).
     */
    val `scriptpubkeyAddress`: kotlin.String?, 
    /**
     * The value in satoshis.
     */
    val `value`: kotlin.Long, 
    /**
     * The output index in the transaction.
     */
    val `n`: kotlin.UInt
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class ValidationResult (
    val `address`: kotlin.String, 
    val `network`: NetworkType, 
    val `addressType`: AddressType
) {
    public companion object
}




@kotlinx.serialization.Serializable
public sealed class Activity {
    @kotlinx.serialization.Serializable
    public data class Onchain(
        val v1: OnchainActivity,
    ) : Activity() {
    }
    @kotlinx.serialization.Serializable
    public data class Lightning(
        val v1: LightningActivity,
    ) : Activity() {
    }
    
}







public sealed class ActivityException: kotlin.Exception() {
    
    public class InvalidActivity(
        public val `errorDetails`: kotlin.String,
    ) : ActivityException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class InitializationException(
        public val `errorDetails`: kotlin.String,
    ) : ActivityException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class InsertException(
        public val `errorDetails`: kotlin.String,
    ) : ActivityException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class RetrievalException(
        public val `errorDetails`: kotlin.String,
    ) : ActivityException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class DataException(
        public val `errorDetails`: kotlin.String,
    ) : ActivityException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class ConnectionException(
        public val `errorDetails`: kotlin.String,
    ) : ActivityException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class SerializationException(
        public val `errorDetails`: kotlin.String,
    ) : ActivityException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
}





@kotlinx.serialization.Serializable
public enum class ActivityFilter {
    
    ALL,
    LIGHTNING,
    ONCHAIN;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class ActivityType {
    
    ONCHAIN,
    LIGHTNING;
    public companion object
}







public sealed class AddressException: kotlin.Exception() {
    
    public class InvalidAddress(
    ) : AddressException() {
        override val message: String
            get() = ""
    }
    
    public class InvalidNetwork(
    ) : AddressException() {
        override val message: String
            get() = ""
    }
    
    public class MnemonicGenerationFailed(
    ) : AddressException() {
        override val message: String
            get() = ""
    }
    
    public class InvalidMnemonic(
    ) : AddressException() {
        override val message: String
            get() = ""
    }
    
    public class InvalidEntropy(
    ) : AddressException() {
        override val message: String
            get() = ""
    }
    
    public class AddressDerivationFailed(
    ) : AddressException() {
        override val message: String
            get() = ""
    }
    
}





@kotlinx.serialization.Serializable
public enum class AddressType {
    
    P2PKH,
    P2SH,
    P2WPKH,
    P2WSH,
    P2TR,
    UNKNOWN;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class BitcoinNetworkEnum {
    
    MAINNET,
    TESTNET,
    SIGNET,
    REGTEST;
    public companion object
}







public sealed class BlocktankException: kotlin.Exception() {
    
    public class HttpClient(
        public val `errorDetails`: kotlin.String,
    ) : BlocktankException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class BlocktankClient(
        public val `errorDetails`: kotlin.String,
    ) : BlocktankException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class InvalidBlocktank(
        public val `errorDetails`: kotlin.String,
    ) : BlocktankException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class InitializationException(
        public val `errorDetails`: kotlin.String,
    ) : BlocktankException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class InsertException(
        public val `errorDetails`: kotlin.String,
    ) : BlocktankException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class RetrievalException(
        public val `errorDetails`: kotlin.String,
    ) : BlocktankException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class DataException(
        public val `errorDetails`: kotlin.String,
    ) : BlocktankException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class ConnectionException(
        public val `errorDetails`: kotlin.String,
    ) : BlocktankException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class SerializationException(
        public val `errorDetails`: kotlin.String,
    ) : BlocktankException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class ChannelOpen(
        public val `errorType`: BtChannelOrderErrorType,
        public val `errorDetails`: kotlin.String,
    ) : BlocktankException() {
        override val message: String
            get() = "errorType=${ `errorType` }, errorDetails=${ `errorDetails` }"
    }
    
    public class OrderState(
        public val `errorDetails`: kotlin.String,
    ) : BlocktankException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class InvalidParameter(
        public val `errorDetails`: kotlin.String,
    ) : BlocktankException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class DatabaseException(
        public val `errorDetails`: kotlin.String,
    ) : BlocktankException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
}





@kotlinx.serialization.Serializable
public enum class BtBolt11InvoiceState {
    
    PENDING,
    HOLDING,
    PAID,
    CANCELED;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class BtChannelOrderErrorType {
    
    WRONG_ORDER_STATE,
    PEER_NOT_REACHABLE,
    CHANNEL_REJECTED_BY_DESTINATION,
    CHANNEL_REJECTED_BY_LSP,
    BLOCKTANK_NOT_READY;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class BtOpenChannelState {
    
    OPENING,
    OPEN,
    CLOSED;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class BtOrderState {
    
    CREATED,
    EXPIRED,
    OPEN,
    CLOSED;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class BtOrderState2 {
    
    CREATED,
    EXPIRED,
    EXECUTED,
    PAID;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class BtPaymentState {
    
    CREATED,
    PARTIALLY_PAID,
    PAID,
    REFUNDED,
    REFUND_AVAILABLE;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class BtPaymentState2 {
    
    CREATED,
    PAID,
    REFUNDED,
    REFUND_AVAILABLE,
    CANCELED;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class CJitStateEnum {
    
    CREATED,
    COMPLETED,
    EXPIRED,
    FAILED;
    public companion object
}







public sealed class DbException: kotlin.Exception() {
    
    public class DbActivityException(
        public val `errorDetails`: ActivityException,
    ) : DbException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class DbBlocktankException(
        public val `errorDetails`: BlocktankException,
    ) : DbException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class InitializationException(
        public val `errorDetails`: kotlin.String,
    ) : DbException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
}





public sealed class DecodingException: kotlin.Exception() {
    
    public class InvalidFormat(
    ) : DecodingException() {
        override val message: String
            get() = ""
    }
    
    public class InvalidNetwork(
    ) : DecodingException() {
        override val message: String
            get() = ""
    }
    
    public class InvalidAmount(
    ) : DecodingException() {
        override val message: String
            get() = ""
    }
    
    public class InvalidLnurlPayAmount(
        public val `amountSatoshis`: kotlin.ULong,
        public val `min`: kotlin.ULong,
        public val `max`: kotlin.ULong,
    ) : DecodingException() {
        override val message: String
            get() = "amountSatoshis=${ `amountSatoshis` }, min=${ `min` }, max=${ `max` }"
    }
    
    public class InvalidTimestamp(
    ) : DecodingException() {
        override val message: String
            get() = ""
    }
    
    public class InvalidChecksum(
    ) : DecodingException() {
        override val message: String
            get() = ""
    }
    
    public class InvalidResponse(
    ) : DecodingException() {
        override val message: String
            get() = ""
    }
    
    public class UnsupportedType(
    ) : DecodingException() {
        override val message: String
            get() = ""
    }
    
    public class InvalidAddress(
    ) : DecodingException() {
        override val message: String
            get() = ""
    }
    
    public class RequestFailed(
    ) : DecodingException() {
        override val message: String
            get() = ""
    }
    
    public class ClientCreationFailed(
    ) : DecodingException() {
        override val message: String
            get() = ""
    }
    
    public class InvoiceCreationFailed(
        public val `errorMessage`: kotlin.String,
    ) : DecodingException() {
        override val message: String
            get() = "errorMessage=${ `errorMessage` }"
    }
    
}





public sealed class LnurlException: kotlin.Exception() {
    
    public class InvalidAddress(
    ) : LnurlException() {
        override val message: String
            get() = ""
    }
    
    public class ClientCreationFailed(
    ) : LnurlException() {
        override val message: String
            get() = ""
    }
    
    public class RequestFailed(
    ) : LnurlException() {
        override val message: String
            get() = ""
    }
    
    public class InvalidResponse(
    ) : LnurlException() {
        override val message: String
            get() = ""
    }
    
    public class InvalidAmount(
        public val `amountSatoshis`: kotlin.ULong,
        public val `min`: kotlin.ULong,
        public val `max`: kotlin.ULong,
    ) : LnurlException() {
        override val message: String
            get() = "amountSatoshis=${ `amountSatoshis` }, min=${ `min` }, max=${ `max` }"
    }
    
    public class InvoiceCreationFailed(
        public val `errorDetails`: kotlin.String,
    ) : LnurlException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class AuthenticationFailed(
    ) : LnurlException() {
        override val message: String
            get() = ""
    }
    
}





@kotlinx.serialization.Serializable
public enum class ManualRefundStateEnum {
    
    CREATED,
    APPROVED,
    REJECTED,
    SENT;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class Network {
    
    /**
     * Mainnet Bitcoin.
     */
    BITCOIN,
    /**
     * Bitcoin's testnet network.
     */
    TESTNET,
    /**
     * Bitcoin's testnet4 network.
     */
    TESTNET4,
    /**
     * Bitcoin's signet network.
     */
    SIGNET,
    /**
     * Bitcoin's regtest network.
     */
    REGTEST;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class NetworkType {
    
    BITCOIN,
    TESTNET,
    REGTEST,
    SIGNET;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class PaymentState {
    
    PENDING,
    SUCCEEDED,
    FAILED;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class PaymentType {
    
    SENT,
    RECEIVED;
    public companion object
}






@kotlinx.serialization.Serializable
public sealed class Scanner {
    @kotlinx.serialization.Serializable
    public data class OnChain(
        val `invoice`: OnChainInvoice,
    ) : Scanner() {
    }
    @kotlinx.serialization.Serializable
    public data class Lightning(
        val `invoice`: LightningInvoice,
    ) : Scanner() {
    }
    @kotlinx.serialization.Serializable
    public data class PubkyAuth(
        val `data`: kotlin.String,
    ) : Scanner() {
    }
    @kotlinx.serialization.Serializable
    public data class LnurlChannel(
        val `data`: LnurlChannelData,
    ) : Scanner() {
    }
    @kotlinx.serialization.Serializable
    public data class LnurlAuth(
        val `data`: LnurlAuthData,
    ) : Scanner() {
    }
    @kotlinx.serialization.Serializable
    public data class LnurlWithdraw(
        val `data`: LnurlWithdrawData,
    ) : Scanner() {
    }
    @kotlinx.serialization.Serializable
    public data class LnurlAddress(
        val `data`: LnurlAddressData,
    ) : Scanner() {
    }
    @kotlinx.serialization.Serializable
    public data class LnurlPay(
        val `data`: LnurlPayData,
    ) : Scanner() {
    }
    @kotlinx.serialization.Serializable
    public data class NodeId(
        val `url`: kotlin.String,
        val `network`: NetworkType,
    ) : Scanner() {
    }
    @kotlinx.serialization.Serializable
    public data class Gift(
        val `code`: kotlin.String,
        val `amount`: kotlin.ULong,
    ) : Scanner() {
    }
    
}







@kotlinx.serialization.Serializable
public enum class SortDirection {
    
    ASC,
    DESC;
    public companion object
}







public sealed class SweepException: kotlin.Exception() {
    
    public class SweepFailed(
        public val v1: kotlin.String,
    ) : SweepException() {
        override val message: String
            get() = "v1=${ v1 }"
    }
    
    public class NoUtxosFound(
    ) : SweepException() {
        override val message: String
            get() = ""
    }
    
    public class InvalidMnemonic(
    ) : SweepException() {
        override val message: String
            get() = ""
    }
    
}




/**
 * Bitcoin network / coin type for Trezor operations.
 */

@kotlinx.serialization.Serializable
public enum class TrezorCoinType {
    
    /**
     * Bitcoin mainnet
     */
    BITCOIN,
    /**
     * Bitcoin testnet
     */
    TESTNET,
    /**
     * Bitcoin signet (treated as testnet by the device)
     */
    SIGNET,
    /**
     * Bitcoin regtest
     */
    REGTEST;
    public companion object
}







/**
 * Trezor-related errors exposed via FFI.
 */
public sealed class TrezorException: kotlin.Exception() {
    
    /**
     * Transport layer error (USB/Bluetooth communication)
     */
    public class TransportException(
        public val `errorDetails`: kotlin.String,
    ) : TrezorException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    /**
     * No Trezor device found
     */
    public class DeviceNotFound(
    ) : TrezorException() {
        override val message: String
            get() = ""
    }
    
    /**
     * Device disconnected during operation
     */
    public class DeviceDisconnected(
    ) : TrezorException() {
        override val message: String
            get() = ""
    }
    
    /**
     * Connection error
     */
    public class ConnectionException(
        public val `errorDetails`: kotlin.String,
    ) : TrezorException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    /**
     * Protocol error (encoding/decoding)
     */
    public class ProtocolException(
        public val `errorDetails`: kotlin.String,
    ) : TrezorException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    /**
     * Pairing required for Bluetooth connection
     */
    public class PairingRequired(
    ) : TrezorException() {
        override val message: String
            get() = ""
    }
    
    /**
     * Pairing failed
     */
    public class PairingFailed(
        public val `errorDetails`: kotlin.String,
    ) : TrezorException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    /**
     * PIN is required
     */
    public class PinRequired(
    ) : TrezorException() {
        override val message: String
            get() = ""
    }
    
    /**
     * PIN entry cancelled
     */
    public class PinCancelled(
    ) : TrezorException() {
        override val message: String
            get() = ""
    }
    
    /**
     * Invalid PIN entered
     */
    public class InvalidPin(
    ) : TrezorException() {
        override val message: String
            get() = ""
    }
    
    /**
     * Passphrase is required
     */
    public class PassphraseRequired(
    ) : TrezorException() {
        override val message: String
            get() = ""
    }
    
    /**
     * Action cancelled by user on device
     */
    public class UserCancelled(
    ) : TrezorException() {
        override val message: String
            get() = ""
    }
    
    /**
     * Operation timed out
     */
    public class Timeout(
    ) : TrezorException() {
        override val message: String
            get() = ""
    }
    
    /**
     * Invalid derivation path
     */
    public class InvalidPath(
        public val `errorDetails`: kotlin.String,
    ) : TrezorException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    /**
     * Device returned an error
     */
    public class DeviceException(
        public val `errorDetails`: kotlin.String,
    ) : TrezorException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    /**
     * Trezor manager not initialized
     */
    public class NotInitialized(
    ) : TrezorException() {
        override val message: String
            get() = ""
    }
    
    /**
     * No device connected
     */
    public class NotConnected(
    ) : TrezorException() {
        override val message: String
            get() = ""
    }
    
    /**
     * Session error
     */
    public class SessionException(
        public val `errorDetails`: kotlin.String,
    ) : TrezorException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    /**
     * IO error
     */
    public class IoException(
        public val `errorDetails`: kotlin.String,
    ) : TrezorException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
}




/**
 * Script types for address derivation.
 */

@kotlinx.serialization.Serializable
public enum class TrezorScriptType {
    
    /**
     * P2PKH (legacy)
     */
    SPEND_ADDRESS,
    /**
     * P2SH-P2WPKH (nested SegWit)
     */
    SPEND_P2SH_WITNESS,
    /**
     * P2WPKH (native SegWit)
     */
    SPEND_WITNESS,
    /**
     * P2TR (Taproot)
     */
    SPEND_TAPROOT,
    /**
     * P2SH multisig
     */
    SPEND_MULTISIG,
    /**
     * External/watch-only input (not signed by device)
     */
    EXTERNAL;
    public companion object
}






/**
 * Transport type for Trezor devices.
 */

@kotlinx.serialization.Serializable
public enum class TrezorTransportType {
    
    /**
     * USB connection
     */
    USB,
    /**
     * Bluetooth connection
     */
    BLUETOOTH;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class WordCount {
    
    /**
     * 12-word mnemonic (128 bits of entropy)
     */
    WORDS12,
    /**
     * 15-word mnemonic (160 bits of entropy)
     */
    WORDS15,
    /**
     * 18-word mnemonic (192 bits of entropy)
     */
    WORDS18,
    /**
     * 21-word mnemonic (224 bits of entropy)
     */
    WORDS21,
    /**
     * 24-word mnemonic (256 bits of entropy)
     */
    WORDS24;
    public companion object
}











































































































































