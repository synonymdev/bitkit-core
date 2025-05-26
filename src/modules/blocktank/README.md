# Blocktank Module

The Blocktank module is responsible for creating, storing and managing LSP info, orders and cjit entries.

## Available Methods

```rust
// Initialize the database with a specified path
fn init_db(base_path: String) -> Result<String, DbError>

// Update the Blocktank URL
async fn update_blocktank_url(new_url: String) -> Result<(), BlocktankError>

// Get service information with optional refresh
async fn get_info(refresh: Option<bool>) -> Result<Option<IBtInfo>, BlocktankError>

// Create a new order
async fn create_order(
    lsp_balance_sat: u64,
    channel_expiry_weeks: u32,
    options: Option<CreateOrderOptions>,
) -> Result<IBtOrder, BlocktankError>

// Open a channel for an order
async fn open_channel(
    order_id: String,
    connection_string: String,
) -> Result<IBtOrder, BlocktankError>

// Get orders with optional filtering
async fn get_orders(
    order_ids: Option<Vec<String>>,
    filter: Option<BtOrderState2>,
    refresh: bool,
) -> Result<Vec<IBtOrder>, BlocktankError>

// Refresh all active orders
async fn refresh_active_orders() -> Result<Vec<IBtOrder>, BlocktankError>

// Get minimum zero-conf transaction fee for an order
async fn get_min_zero_conf_tx_fee(
    order_id: String,
) -> Result<IBt0ConfMinTxFeeWindow, BlocktankError>

// Estimate order fee
async fn estimate_order_fee(
    lsp_balance_sat: u64,
    channel_expiry_weeks: u32,
    options: Option<CreateOrderOptions>,
) -> Result<IBtEstimateFeeResponse, BlocktankError>

// Estimate order fee with full breakdown
async fn estimate_order_fee_full(
    lsp_balance_sat: u64,
    channel_expiry_weeks: u32,
    options: Option<CreateOrderOptions>,
) -> Result<IBtEstimateFeeResponse2, BlocktankError>

// Create a CJIT entry
async fn create_cjit_entry(
    channel_size_sat: u64,
    invoice_sat: u64,
    invoice_description: String,
    node_id: String,
    channel_expiry_weeks: u32,
    options: Option<CreateCjitOptions>,
) -> Result<ICJitEntry, BlocktankError>

// Get CJIT entries with optional filtering
async fn get_cjit_entries(
    entry_ids: Option<Vec<String>>,
    filter: Option<CJitStateEnum>,
    refresh: bool,
) -> Result<Vec<ICJitEntry>, BlocktankError>

// Refresh all active CJIT entries
async fn refresh_active_cjit_entries() -> Result<Vec<ICJitEntry>, BlocktankError>

// Register a device for notifications
async fn register_device(
    device_token: String,
    public_key: String,
    features: Vec<String>,
    node_id: String,
    iso_timestamp: String,
    signature: String,
    is_production: Option<bool>,
    custom_url: Option<String>
) -> Result<String, BlocktankError>

// Send a test notification to a registered device
async fn test_notification(
    device_token: String,
    secret_message: String,
    notification_type: Option<String>,
    custom_url: Option<String>
) -> Result<String, BlocktankError>
// Mine blocks in regtest mode
async fn regtest_mine(count: Option<u32>) -> Result<(), BlocktankError>

// Deposit funds to an address in regtest mode
async fn regtest_deposit(
    address: String,
    amount_sat: Option<u64>,
) -> Result<String, BlocktankError>

// Pay an invoice in regtest mode
async fn regtest_pay(
    invoice: String,
    amount_sat: Option<u64>,
) -> Result<String, BlocktankError>

// Get payment information in regtest mode
async fn regtest_get_payment(payment_id: String) -> Result<IBtBolt11Invoice, BlocktankError>

// Close a channel in regtest mode
async fn regtest_close_channel(
    funding_tx_id: String,
    vout: u32,
    force_close_after_s: Option<u64>,
) -> Result<String, BlocktankError>
```

## Usage Examples

### iOS (Swift)
```swift
import BitkitCore

func manageBlocktank() async {
    do {
        // Initialize database
        try initDb("/path/to/data")  // Creates /path/to/data/blocktank.db
        
        // Update Blocktank URL if needed
        try await updateBlocktankUrl("https://api1.blocktank.to/api")
        
        // Get cached info
        if let info = try await getInfo(refresh: false) {
            print("Current LSP nodes: \(info.nodes.count)")
            print("Network: \(info.onchain.network)")
            print("Fee rates: Fast \(info.onchain.feeRates.fast)")
        }
        
        // Create an order
        let options = CreateOrderOptions(
            clientBalanceSat: 50000,
            turboChannel: true,
            zeroReserve: true
        )
        let order = try await createOrder(
            lspBalanceSat: 100000,
            channelExpiryWeeks: 4,
            options: options
        )
        print("Created order: \(order.id)")
        
        // Estimate fees
        let feeEstimate = try await estimateOrderFeeFull(
            lspBalanceSat: 100000,
            channelExpiryWeeks: 4,
            options: options
        )
        print("Estimated fees: \(feeEstimate.feeSat) sats")
        print("Network fee: \(feeEstimate.networkFeeSat) sats")
        print("Service fee: \(feeEstimate.serviceFeeSat) sats")
        
        // Open channel
        let openedOrder = try await openChannel(
            orderId: order.id,
            connectionString: "node_pubkey@host:port"
        )
        print("Channel state: \(openedOrder.channel?.state ?? "unknown")")
        
        let activeCjitEntries = try await refreshActiveCjitEntries()
        print("Active CJIT entries: \(activeCjitEntries.count)")
        
        // Register a device for notifications
        let deviceToken = "example-device-token"
        let publicKey = "device-public-key"
        let features = ["lightning", "channel"]
        let nodeId = "node-id"
        let timestamp = ISO8601DateFormatter().string(from: Date())
        let signature = "signature-of-timestamp-with-node-private-key"
        
        let registrationResult = try await registerDevice(
            deviceToken: deviceToken,
            publicKey: publicKey,
            features: features,
            nodeId: nodeId,
            isoTimestamp: timestamp,
            signature: signature,
            isProduction: true,
            customUrl: nil
        )
        print("Device registration result: \(registrationResult)")
        
        // Send a test notification
        let notificationResult = try await testNotification(
            deviceToken: deviceToken,
            secretMessage: "Test notification message"
        )
        print("Test notification result: \(notificationResult)")
        
        // For testing/development in regtest mode
        try await regtestMine(count: 6)  // Mine 6 blocks
        
        let depositTxid = try await regtestDeposit(
            address: "bcrt1...",
            amountSat: 100000
        )
        print("Deposited funds, txid: \(depositTxid)")
        
        let paymentId = try await regtestPay(
            invoice: "lnbcrt...",
            amountSat: 10000
        )
        print("Payment made, id: \(paymentId)")
        
        let paymentInfo = try await regtestGetPayment(paymentId: paymentId)
        print("Payment status: \(paymentInfo.status)")
        
        let closeTxid = try await regtestCloseChannel(
            fundingTxId: "txid",
            vout: 0,
            forceCloseAfterS: 60
        )
        print("Channel closed, txid: \(closeTxid)")
        
    } catch {
        print("Error: \(error)")
    }
}
```

### Android (Kotlin)
```kotlin
import com.synonym.bitkitcore.*

suspend fun manageBlocktank() {
    try {
        // Initialize database
        initDb("/path/to/data")  // Creates /path/to/data/blocktank.db
        
        // Update Blocktank URL
        updateBlocktankUrl("https://api1.blocktank.to/api")
        
        // Get cached info
        getInfo(refresh = false)?.let { info ->
            println("Current LSP nodes: ${info.nodes.size}")
            println("Network: ${info.onchain.network}")
        }
        
        // Create a CJIT entry
        val cjitOptions = CreateCjitOptions(
            source = "android-app"
        )
        val cjit = createCjitEntry(
            channelSizeSat = 100000,
            invoiceSat = 50000,
            invoiceDescription = "Channel creation",
            nodeId = "node_pubkey",
            channelExpiryWeeks = 4,
            options = cjitOptions
        )
        println("Created CJIT entry: ${cjit.id}")
        
        // Get active orders
        val activeOrders = refreshActiveOrders()
        println("Active orders: ${activeOrders.size}")
        
        // Get CJIT entries
        val entries = getCjitEntries(
            entryIds = null,
            filter = CJitStateEnum.CREATED,
            refresh = true
        )
        println("CJIT entries: ${entries.size}")
        
        val activeCjitEntries = refreshActiveCjitEntries()
        println("Active CJIT entries: ${activeCjitEntries.size}")
        
        // Register device for notifications
        val deviceToken = "example-device-token"
        val publicKey = "device-public-key"
        val features = listOf("lightning", "channel")
        val nodeId = "node-id"
        val timestamp = java.time.format.DateTimeFormatter.ISO_INSTANT.format(java.time.Instant.now())
        val signature = "signature-of-timestamp-with-node-private-key"
        
        val registrationResult = registerDevice(
            deviceToken = deviceToken,
            publicKey = publicKey,
            features = features,
            nodeId = nodeId,
            isoTimestamp = timestamp,
            signature = signature,
            isProduction = true,
            customUrl = null
        )
        println("Device registration result: $registrationResult")
        
        // Send a test notification
        val notificationResult = testNotification(
            deviceToken = deviceToken,
            secretMessage = "Test notification message"
        )
        println("Test notification result: $notificationResult")
        
        // For regtest mode testing
        regtestMine(count = 6)  // Mine 6 blocks
        
        val depositTxid = regtestDeposit(
            address = "bcrt1...",
            amountSat = 100000
        )
        println("Deposited funds, txid: $depositTxid")
        
        val paymentId = regtestPay(
            invoice = "lnbcrt...",
            amountSat = 10000
        )
        println("Payment made, id: $paymentId")
        
        val paymentInfo = regtestGetPayment(paymentId = paymentId)
        println("Payment status: ${paymentInfo.status}")
        
        val closeTxid = regtestCloseChannel(
            fundingTxId = "txid",
            vout = 0,
            forceCloseAfterS = 60
        )
        println("Channel closed, txid: $closeTxid")
        
    } catch (e: Exception) {
        println("Error: $e")
    }
}
```

### Python
```python
from bitkitcore import *
import asyncio
from datetime import datetime, timezone

async def manage_blocktank():
    try:
        # Initialize database
        init_db("/path/to/data")  # Creates /path/to/data/blocktank.db
        
        # Update Blocktank URL
        await update_blocktank_url("https://api1.blocktank.to/api")
        
        # Get cached info
        if info := await get_info(refresh=False):
            print(f"Current LSP nodes: {len(info.nodes)}")
            print(f"Network: {info.onchain.network}")
        
        # Create an order
        options = CreateOrderOptions(
            client_balance_sat=50000,
            zero_conf=True,
            zero_reserve=True
        )
        order = await create_order(
            lsp_balance_sat=100000,
            channel_expiry_weeks=4,
            options=options
        )
        print(f"Created order: {order.id}")
        
        # Get min zero-conf fee
        fee_window = await get_min_zero_conf_tx_fee(order.id)
        print(f"Min fee rate: {fee_window.sat_per_vbyte} sat/vbyte")
        print(f"Valid until: {fee_window.validity_ends_at}")
        
        # Get orders by state
        orders = await get_orders(
            order_ids=None,
            filter=BtOrderState2.CREATED,
            refresh=True
        )
        print(f"Created orders: {len(orders)}")
        
        active_cjit_entries = await refresh_active_cjit_entries()
        print(f"Active CJIT entries: {len(active_cjit_entries)}")
        
        # Register a device for notifications
        device_token = "example-device-token"
        public_key = "device-public-key"
        features = ["lightning", "channel"]
        node_id = "node-id"
        timestamp = datetime.now(timezone.utc).isoformat()
        signature = "signature-of-timestamp-with-node-private-key"
        
        registration_result = await register_device(
            device_token=device_token,
            public_key=public_key,
            features=features,
            node_id=node_id,
            iso_timestamp=timestamp,
            signature=signature,
            is_production=True,
            custom_url=None
        )
        print(f"Device registration result: {registration_result}")
        
        # Send a test notification
        notification_result = await test_notification(
            device_token=device_token,
            secret_message="Test notification message"
        )
        print(f"Test notification result: {notification_result}")
        
        # For regtest mode testing
        await regtest_mine(count=6)  # Mine 6 blocks
        
        deposit_txid = await regtest_deposit(
            address="bcrt1...",
            amount_sat=100000
        )
        print(f"Deposited funds, txid: {deposit_txid}")
        
        payment_id = await regtest_pay(
            invoice="lnbcrt...",
            amount_sat=10000
        )
        print(f"Payment made, id: {payment_id}")
        
        payment_info = await regtest_get_payment(payment_id=payment_id)
        print(f"Payment status: {payment_info.status}")
        
        close_txid = await regtest_close_channel(
            funding_tx_id="txid",
            vout=0,
            force_close_after_s=60
        )
        print(f"Channel closed, txid: {close_txid}")
        
    except Exception as e:
        print(f"Error: {e}")

if __name__ == "__main__":
    asyncio.run(manage_blocktank())
```