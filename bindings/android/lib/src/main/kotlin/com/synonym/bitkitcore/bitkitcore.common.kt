

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



/**
 * Account info response
 */
@kotlinx.serialization.Serializable
public data class AccountInfoResponse (
    val `id`: kotlin.UInt, 
    val `path`: kotlin.String, 
    val `descriptor`: kotlin.String, 
    val `legacyXpub`: kotlin.String?, 
    val `balance`: kotlin.String, 
    val `availableBalance`: kotlin.String
) {
    public companion object
}



/**
 * UTXO information for account
 */
@kotlinx.serialization.Serializable
public data class AccountUtxo (
    /**
     * Transaction ID
     */
    val `txid`: kotlin.String, 
    /**
     * Output index
     */
    val `vout`: kotlin.UInt, 
    /**
     * Amount in satoshis
     */
    val `amount`: kotlin.String, 
    /**
     * Block height
     */
    val `blockHeight`: kotlin.UInt?, 
    /**
     * Address
     */
    val `address`: kotlin.String, 
    /**
     * Derivation path
     */
    val `path`: kotlin.String, 
    /**
     * Number of confirmations
     */
    val `confirmations`: kotlin.UInt?
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



/**
 * Address response containing the derived address information
 */
@kotlinx.serialization.Serializable
public data class AddressResponse (
    val `address`: kotlin.String, 
    val `path`: List<kotlin.UInt>, 
    val `serializedPath`: kotlin.String
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



/**
 * Coin purchase memo
 */
@kotlinx.serialization.Serializable
public data class CoinPurchaseMemo (
    /**
     * Coin type
     */
    val `coinType`: kotlin.UInt, 
    /**
     * Amount
     */
    val `amount`: kotlin.ULong, 
    /**
     * Address
     */
    val `address`: kotlin.String, 
    /**
     * MAC
     */
    val `mac`: kotlin.String
) {
    public companion object
}



/**
 * Common parameters for all Trezor Connect methods
 */
@kotlinx.serialization.Serializable
public data class CommonParams (
    /**
     * Specific device instance to use
     */
    val `device`: DeviceParams?, 
    /**
     * Set to true if method should use empty passphrase
     */
    val `useEmptyPassphrase`: kotlin.Boolean?, 
    /**
     * Allow seedless device
     */
    val `allowSeedlessDevice`: kotlin.Boolean?, 
    /**
     * Skip final reload
     */
    val `skipFinalReload`: kotlin.Boolean?
) {
    public companion object
}



/**
 * Account information for compose transaction
 */
@kotlinx.serialization.Serializable
public data class ComposeAccount (
    /**
     * Derivation path
     */
    val `path`: kotlin.String, 
    /**
     * Account addresses
     */
    val `addresses`: AccountAddresses, 
    /**
     * UTXOs
     */
    val `utxo`: List<AccountUtxo>
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



/**
 * Result type for deep link generation, including the URL and the ID used
 */
@kotlinx.serialization.Serializable
public data class DeepLinkResult (
    /**
     * The generated deep link URL
     */
    val `url`: kotlin.String, 
    /**
     * The request ID used (either provided or auto-generated)
     */
    val `requestId`: kotlin.String
) {
    public companion object
}



/**
 * Parameters for specifying a particular device
 */
@kotlinx.serialization.Serializable
public data class DeviceParams (
    /**
     * Device instance path
     */
    val `path`: kotlin.String?, 
    /**
     * Device instance ID
     */
    val `instance`: kotlin.UInt?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class ErrorData (
    val `errorDetails`: kotlin.String
) {
    public companion object
}



/**
 * Feature response containing device capabilities and information
 */
@kotlinx.serialization.Serializable
public data class FeatureResponse (
    val `vendor`: kotlin.String, 
    val `majorVersion`: kotlin.UInt, 
    val `minorVersion`: kotlin.UInt, 
    val `patchVersion`: kotlin.UInt, 
    val `deviceId`: kotlin.String, 
    val `capabilities`: List<kotlin.String>?
) {
    public companion object
}



/**
 * Fee level for compose transaction
 */
@kotlinx.serialization.Serializable
public data class FeeLevel (
    /**
     * Fee per unit (satoshi/byte or satoshi/vbyte)
     */
    val `feePerUnit`: kotlin.String, 
    /**
     * Base fee in satoshi (optional, used in RBF and DOGE)
     */
    val `baseFee`: kotlin.UInt?, 
    /**
     * Floor base fee (optional, used in DOGE)
     */
    val `floorBaseFee`: kotlin.Boolean?
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



/**
 * HD Node Path Type
 */
@kotlinx.serialization.Serializable
public data class HdNodePathType (
    /**
     * Node data (can be String or HDNodeType)
     */
    val `node`: HdNodeTypeOrString, 
    /**
     * BIP32 derivation path
     */
    val `addressN`: List<kotlin.UInt>
) {
    public companion object
}



/**
 * HD Node Type
 */
@kotlinx.serialization.Serializable
public data class HdNodeType (
    /**
     * Depth
     */
    val `depth`: kotlin.UInt, 
    /**
     * Fingerprint
     */
    val `fingerprint`: kotlin.UInt, 
    /**
     * Child number
     */
    val `childNum`: kotlin.UInt, 
    /**
     * Chain code
     */
    val `chainCode`: kotlin.String, 
    /**
     * Public key
     */
    val `publicKey`: kotlin.String, 
    /**
     * Private key (optional)
     */
    val `privateKey`: kotlin.String?, 
    /**
     * BIP32 derivation path (optional)
     */
    val `addressN`: List<kotlin.UInt>?
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
    val `updatedAt`: kotlin.ULong?
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
 * Message signature response
 */
@kotlinx.serialization.Serializable
public data class MessageSignatureResponse (
    /**
     * Signer address
     */
    val `address`: kotlin.String, 
    /**
     * Signature in base64 format
     */
    val `signature`: kotlin.String
) {
    public companion object
}



/**
 * Multisig Redeem Script Type
 */
@kotlinx.serialization.Serializable
public data class MultisigRedeemScriptType (
    /**
     * Public keys
     */
    val `pubkeys`: List<HdNodePathType>, 
    /**
     * Signatures
     */
    val `signatures`: List<kotlin.String>, 
    /**
     * M-of-N threshold
     */
    val `m`: kotlin.UInt, 
    /**
     * Nodes (optional)
     */
    val `nodes`: List<HdNodeType>?, 
    /**
     * Pubkeys order (optional): 0 for PRESERVED, 1 for LEXICOGRAPHIC
     */
    val `pubkeysOrder`: kotlin.UByte?
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
    val `updatedAt`: kotlin.ULong?
) {
    public companion object
}



/**
 * Payment request memo types
 */
@kotlinx.serialization.Serializable
public data class PaymentRequestMemo (
    /**
     * Text memo
     */
    val `textMemo`: TextMemo?, 
    /**
     * Refund memo
     */
    val `refundMemo`: RefundMemo?, 
    /**
     * Coin purchase memo
     */
    val `coinPurchaseMemo`: CoinPurchaseMemo?
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



/**
 * Precomposed transaction input
 */
@kotlinx.serialization.Serializable
public data class PrecomposedInput (
    /**
     * BIP32 derivation path
     */
    val `addressN`: List<kotlin.UInt>, 
    /**
     * Amount in satoshis
     */
    val `amount`: kotlin.String, 
    /**
     * Previous transaction hash
     */
    val `prevHash`: kotlin.String, 
    /**
     * Previous output index
     */
    val `prevIndex`: kotlin.UInt, 
    /**
     * Script type
     */
    val `scriptType`: ScriptType
) {
    public companion object
}



/**
 * Precomposed transaction output
 */
@kotlinx.serialization.Serializable
public data class PrecomposedOutput (
    /**
     * BIP32 derivation path (for change outputs)
     */
    val `addressN`: List<kotlin.UInt>?, 
    /**
     * Amount in satoshis
     */
    val `amount`: kotlin.String, 
    /**
     * Address (for regular outputs)
     */
    val `address`: kotlin.String?, 
    /**
     * Script type
     */
    val `scriptType`: ScriptType
) {
    public companion object
}



/**
 * Precomposed transaction
 */
@kotlinx.serialization.Serializable
public data class PrecomposedTransaction (
    /**
     * Transaction type (usually "final" or "error")
     */
    val `txType`: kotlin.String, 
    /**
     * Total amount spent (including fee)
     */
    val `totalSpent`: kotlin.String?, 
    /**
     * Transaction fee
     */
    val `fee`: kotlin.String?, 
    /**
     * Fee per byte
     */
    val `feePerByte`: kotlin.String?, 
    /**
     * Transaction size in bytes
     */
    val `bytes`: kotlin.UInt?, 
    /**
     * Transaction inputs
     */
    val `inputs`: List<PrecomposedInput>?, 
    /**
     * Transaction outputs
     */
    val `outputs`: List<PrecomposedOutput>?, 
    /**
     * Output permutation indices
     */
    val `outputsPermutation`: List<kotlin.UInt>?
) {
    public companion object
}



@kotlinx.serialization.Serializable
public data class PubkyAuth (
    val `data`: kotlin.String
) {
    public companion object
}



/**
 * Public key response containing the derived public key information
 */
@kotlinx.serialization.Serializable
public data class PublicKeyResponse (
    val `path`: List<kotlin.UInt>, 
    val `serializedPath`: kotlin.String, 
    val `xpub`: kotlin.String, 
    val `xpubSegwit`: kotlin.String?, 
    val `chainCode`: kotlin.String, 
    val `childNum`: kotlin.UInt, 
    val `publicKey`: kotlin.String, 
    val `fingerprint`: kotlin.UInt, 
    val `depth`: kotlin.UInt, 
    val `descriptor`: kotlin.String?
) {
    public companion object
}



/**
 * Reference transaction for transaction signing
 */
@kotlinx.serialization.Serializable
public data class RefTransaction (
    /**
     * Transaction hash
     */
    val `hash`: kotlin.String, 
    /**
     * Transaction version
     */
    val `version`: kotlin.UInt?, 
    /**
     * Transaction inputs
     */
    val `inputs`: List<RefTxInput>, 
    /**
     * Transaction outputs (binary format)
     */
    val `binOutputs`: List<RefTxOutput>, 
    /**
     * Lock time
     */
    val `lockTime`: kotlin.UInt?, 
    /**
     * Expiry (for Zcash/Decred)
     */
    val `expiry`: kotlin.UInt?, 
    /**
     * Version group ID (for Zcash)
     */
    val `versionGroupId`: kotlin.UInt?, 
    /**
     * Overwintered flag (for Zcash)
     */
    val `overwintered`: kotlin.Boolean?, 
    /**
     * Timestamp (for Capricoin)
     */
    val `timestamp`: kotlin.UInt?, 
    /**
     * Branch ID (for Zcash)
     */
    val `branchId`: kotlin.UInt?, 
    /**
     * Extra data
     */
    val `extraData`: kotlin.String?
) {
    public companion object
}



/**
 * Reference transaction input
 */
@kotlinx.serialization.Serializable
public data class RefTxInput (
    /**
     * Previous transaction hash
     */
    val `prevHash`: kotlin.String, 
    /**
     * Previous transaction output index
     */
    val `prevIndex`: kotlin.UInt, 
    /**
     * Script signature
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
 * Reference transaction output (binary format)
 */
@kotlinx.serialization.Serializable
public data class RefTxOutput (
    /**
     * Amount in satoshis
     */
    val `amount`: kotlin.ULong, 
    /**
     * Script public key (binary hex)
     */
    val `scriptPubkey`: kotlin.String
) {
    public companion object
}



/**
 * Refund memo
 */
@kotlinx.serialization.Serializable
public data class RefundMemo (
    /**
     * Refund address
     */
    val `address`: kotlin.String, 
    /**
     * MAC
     */
    val `mac`: kotlin.String
) {
    public companion object
}



/**
 * Signed transaction response
 */
@kotlinx.serialization.Serializable
public data class SignedTransactionResponse (
    /**
     * Array of signer signatures
     */
    val `signatures`: List<kotlin.String>, 
    /**
     * Serialized transaction
     */
    val `serializedTx`: kotlin.String, 
    /**
     * Broadcasted transaction ID (if push was true)
     */
    val `txid`: kotlin.String?
) {
    public companion object
}



/**
 * Text memo
 */
@kotlinx.serialization.Serializable
public data class TextMemo (
    /**
     * Text content
     */
    val `text`: kotlin.String
) {
    public companion object
}



/**
 * Payment request
 */
@kotlinx.serialization.Serializable
public data class TxAckPaymentRequest (
    /**
     * Nonce
     */
    val `nonce`: kotlin.String?, 
    /**
     * Recipient name
     */
    val `recipientName`: kotlin.String, 
    /**
     * Memos
     */
    val `memos`: List<PaymentRequestMemo>?, 
    /**
     * Amount
     */
    val `amount`: kotlin.ULong?, 
    /**
     * Signature
     */
    val `signature`: kotlin.String
) {
    public companion object
}



/**
 * Transaction input type
 */
@kotlinx.serialization.Serializable
public data class TxInputType (
    /**
     * Previous transaction hash
     */
    val `prevHash`: kotlin.String, 
    /**
     * Previous transaction output index
     */
    val `prevIndex`: kotlin.UInt, 
    /**
     * Amount in satoshis
     */
    val `amount`: kotlin.ULong, 
    /**
     * Transaction sequence
     */
    val `sequence`: kotlin.UInt?, 
    /**
     * BIP32 derivation path
     */
    val `addressN`: List<kotlin.UInt>?, 
    /**
     * Script type
     */
    val `scriptType`: ScriptType?, 
    /**
     * Multisig information
     */
    val `multisig`: MultisigRedeemScriptType?, 
    /**
     * Script public key (for external inputs)
     */
    val `scriptPubkey`: kotlin.String?, 
    /**
     * Script signature
     */
    val `scriptSig`: kotlin.String?, 
    /**
     * Witness data
     */
    val `witness`: kotlin.String?, 
    /**
     * Ownership proof
     */
    val `ownershipProof`: kotlin.String?, 
    /**
     * Commitment data
     */
    val `commitmentData`: kotlin.String?, 
    /**
     * Original hash for RBF
     */
    val `origHash`: kotlin.String?, 
    /**
     * Original index for RBF
     */
    val `origIndex`: kotlin.UInt?, 
    /**
     * Coinjoin flags
     */
    val `coinjoinFlags`: kotlin.UInt?
) {
    public companion object
}



/**
 * Transaction output type
 */
@kotlinx.serialization.Serializable
public data class TxOutputType (
    /**
     * Output address (for address outputs)
     */
    val `address`: kotlin.String?, 
    /**
     * BIP32 derivation path (for change outputs)
     */
    val `addressN`: List<kotlin.UInt>?, 
    /**
     * Amount in satoshis
     */
    val `amount`: kotlin.ULong, 
    /**
     * Script type
     */
    val `scriptType`: ScriptType, 
    /**
     * Multisig information
     */
    val `multisig`: MultisigRedeemScriptType?, 
    /**
     * OP_RETURN data
     */
    val `opReturnData`: kotlin.String?, 
    /**
     * Original hash for RBF
     */
    val `origHash`: kotlin.String?, 
    /**
     * Original index for RBF
     */
    val `origIndex`: kotlin.UInt?, 
    /**
     * Payment request index
     */
    val `paymentReqIndex`: kotlin.UInt?
) {
    public companion object
}



/**
 * Unlock Path parameters
 */
@kotlinx.serialization.Serializable
public data class UnlockPath (
    /**
     * BIP32 derivation path
     */
    val `addressN`: List<kotlin.UInt>, 
    /**
     * MAC (optional)
     */
    val `mac`: kotlin.String?
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



/**
 * Verify message response
 */
@kotlinx.serialization.Serializable
public data class VerifyMessageResponse (
    /**
     * Verification result message
     */
    val `message`: kotlin.String
) {
    public companion object
}



/**
 * Marker object for XRP accounts
 */
@kotlinx.serialization.Serializable
public data class XrpMarker (
    /**
     * Ledger number
     */
    val `ledger`: kotlin.ULong, 
    /**
     * Sequence number
     */
    val `seq`: kotlin.ULong
) {
    public companion object
}




/**
 * Level of details to be returned by getAccountInfo
 */

@kotlinx.serialization.Serializable
public enum class AccountInfoDetails {
    
    /**
     * Return only account balances (default)
     */
    BASIC,
    /**
     * Return with derived addresses or ERC20 tokens
     */
    TOKENS,
    /**
     * Same as tokens with balances
     */
    TOKEN_BALANCES,
    /**
     * TokenBalances + complete account transaction history
     */
    TXS;
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






/**
 * Amount unit for display
 */

@kotlinx.serialization.Serializable
public enum class AmountUnit {
    
    BITCOIN,
    MILLI_BITCOIN,
    MICRO_BITCOIN,
    SATOSHI;
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






/**
 * Output type for compose transaction
 */
@kotlinx.serialization.Serializable
public sealed class ComposeOutput {
    
    /**
     * Regular output with amount and address
     */@kotlinx.serialization.Serializable
    public data class Regular(
        /**
         * Amount in satoshis
         */
        val `amount`: kotlin.String,
        /**
         * Recipient address
         */
        val `address`: kotlin.String,
    ) : ComposeOutput() {
    }
    
    /**
     * Send max output
     */@kotlinx.serialization.Serializable
    public data class SendMax(
        /**
         * Recipient address
         */
        val `address`: kotlin.String,
    ) : ComposeOutput() {
    }
    
    /**
     * OP_RETURN output
     */@kotlinx.serialization.Serializable
    public data class OpReturn(
        /**
         * Hexadecimal string with arbitrary data
         */
        val `dataHex`: kotlin.String,
    ) : ComposeOutput() {
    }
    
    /**
     * Payment without address (precompose only)
     */@kotlinx.serialization.Serializable
    public data class PaymentNoAddress(
        /**
         * Amount in satoshis
         */
        val `amount`: kotlin.String,
    ) : ComposeOutput() {
    }
    
    /**
     * Send max without address (precompose only)
     */
    @kotlinx.serialization.Serializable
    public data object SendMaxNoAddress : ComposeOutput() 
    
    
}






/**
 * Compose transaction response
 */
@kotlinx.serialization.Serializable
public sealed class ComposeTransactionResponse {
    
    /**
     * Signed transaction (payment mode)
     */@kotlinx.serialization.Serializable
    public data class SignedTransaction(
        val v1: SignedTransactionResponse,
    ) : ComposeTransactionResponse() {
    }
    
    /**
     * Precomposed transactions (precompose mode)
     */@kotlinx.serialization.Serializable
    public data class PrecomposedTransactions(
        val v1: List<PrecomposedTransaction>,
    ) : ComposeTransactionResponse() {
    }
    
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




/**
 * Bitcoin account types for default display
 */

@kotlinx.serialization.Serializable
public enum class DefaultAccountType {
    
    /**
     * Normal account
     */
    NORMAL,
    /**
     * SegWit account
     */
    SEGWIT,
    /**
     * Legacy account
     */
    LEGACY;
    public companion object
}






/**
 * Union type for HD Node (either a String or HDNodeType)
 */
@kotlinx.serialization.Serializable
public sealed class HdNodeTypeOrString {
    
    /**
     * HD Node as a string
     */@kotlinx.serialization.Serializable
    public data class String(
        val v1: kotlin.String,
    ) : HdNodeTypeOrString() {
    }
    
    /**
     * HD Node as an object
     */@kotlinx.serialization.Serializable
    public data class Node(
        val v1: HdNodeType,
    ) : HdNodeTypeOrString() {
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






/**
 * Script type for inputs and outputs
 */

@kotlinx.serialization.Serializable
public enum class ScriptType {
    
    SPEND_ADDRESS,
    SPEND_MULTISIG,
    SPEND_WITNESS,
    SPEND_P2SH_WITNESS,
    SPEND_TAPROOT,
    EXTERNAL,
    PAY_TO_ADDRESS,
    PAY_TO_SCRIPT_HASH,
    PAY_TO_MULTISIG,
    PAY_TO_WITNESS,
    PAY_TO_P2SH_WITNESS,
    PAY_TO_TAPROOT,
    PAY_TO_OP_RETURN;
    public companion object
}







@kotlinx.serialization.Serializable
public enum class SortDirection {
    
    ASC,
    DESC;
    public companion object
}






/**
 * Token filter options for getAccountInfo
 */

@kotlinx.serialization.Serializable
public enum class TokenFilter {
    
    /**
     * Return only addresses with nonzero balance (default)
     */
    NONZERO,
    /**
     * Return addresses with at least one transaction
     */
    USED,
    /**
     * Return all derived addresses
     */
    DERIVED;
    public companion object
}







/**
 * Error types for Trezor Connect operations
 */
public sealed class TrezorConnectException: kotlin.Exception() {
    
    /**
     * Error during serialization/deserialization
     */
    public class SerdeException(
        public val `errorDetails`: kotlin.String,
    ) : TrezorConnectException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    /**
     * Error with URL parsing or formatting
     */
    public class UrlException(
        public val `errorDetails`: kotlin.String,
    ) : TrezorConnectException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    /**
     * Environment-related errors
     */
    public class EnvironmentException(
        public val `errorDetails`: kotlin.String,
    ) : TrezorConnectException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    /**
     * General errors
     */
    public class Other(
        public val `errorDetails`: kotlin.String,
    ) : TrezorConnectException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
    public class ClientException(
        public val `errorDetails`: kotlin.String,
    ) : TrezorConnectException() {
        override val message: String
            get() = "errorDetails=${ `errorDetails` }"
    }
    
}




/**
 * Environment options for Trezor deep linking
 */

@kotlinx.serialization.Serializable
public enum class TrezorEnvironment {
    
    /**
     * Production environment (currently unavailable according to docs)
     */
    PRODUCTION,
    /**
     * Development environment
     */
    DEVELOPMENT,
    /**
     * Local environment
     */
    LOCAL;
    public companion object
}






/**
 * Enum representing the different types of Trezor responses
 */
@kotlinx.serialization.Serializable
public sealed class TrezorResponsePayload {
    
    /**
     * Response from getFeatures method
     */@kotlinx.serialization.Serializable
    public data class Features(
        val v1: FeatureResponse,
    ) : TrezorResponsePayload() {
    }
    
    /**
     * Response from getAddress method
     */@kotlinx.serialization.Serializable
    public data class Address(
        val v1: AddressResponse,
    ) : TrezorResponsePayload() {
    }
    
    /**
     * Response from getPublicKey method
     */@kotlinx.serialization.Serializable
    public data class PublicKey(
        val v1: PublicKeyResponse,
    ) : TrezorResponsePayload() {
    }
    
    /**
     * Response from getAccountInfo method
     */@kotlinx.serialization.Serializable
    public data class AccountInfo(
        val v1: AccountInfoResponse,
    ) : TrezorResponsePayload() {
    }
    
    /**
     * Response from composeTransaction method
     */@kotlinx.serialization.Serializable
    public data class ComposeTransaction(
        val v1: ComposeTransactionResponse,
    ) : TrezorResponsePayload() {
    }
    
    /**
     * Response from verifyMessage method
     */@kotlinx.serialization.Serializable
    public data class VerifyMessage(
        val v1: VerifyMessageResponse,
    ) : TrezorResponsePayload() {
    }
    
    /**
     * Response from signMessage method
     */@kotlinx.serialization.Serializable
    public data class MessageSignature(
        val v1: MessageSignatureResponse,
    ) : TrezorResponsePayload() {
    }
    
    /**
     * Response from signTransaction method
     */@kotlinx.serialization.Serializable
    public data class SignedTransaction(
        val v1: SignedTransactionResponse,
    ) : TrezorResponsePayload() {
    }
    
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























































































































































































