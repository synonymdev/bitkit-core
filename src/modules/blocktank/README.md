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
        
    } catch (e: Exception) {
        println("Error: $e")
    }
}
```

### Python
```python
from bitkitcore import *
import asyncio

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
            turbo_channel=True,
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
        
    except Exception as e:
        print(f"Error: {e}")

if __name__ == "__main__":
    asyncio.run(manage_blocktank())
```