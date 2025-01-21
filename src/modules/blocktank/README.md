# Blocktank Module

The Blocktank module is responsible for creating, storing and managing LSP info, orders and cjit entries.

## Available Methods

```rust
// Initialize the database with a specified path
fn init_db(base_path: String) -> Result<String, DbError>

// Update the Blocktank URL
fn update_blocktank_url(new_url: String) -> Result<(), BlocktankError>

// Get service information with optional refresh
fn get_info(refresh: Option<bool>) -> Result<Option<IBtInfo>, BlocktankError>
```

## Usage Examples

### iOS (Swift)
```swift
import BitkitCore

func manageBlocktank() {
    do {
        // Initialize database
        try initDb("/path/to/data")  // Creates /path/to/data/blocktank.db
        
        // Update Blocktank URL if needed
        try updateBlocktankUrl("https://api1.blocktank.to/api")
        
        // Get cached info
        if let info = try getInfo(refresh: false) {
            print("Current LSP nodes: \(info.nodes.count)")
            print("Network: \(info.onchain.network)")
            print("Fee rates: Fast \(info.onchain.feeRates.fast), Mid \(info.onchain.feeRates.mid)")
        }
        
        // Get fresh info from service
        if let freshInfo = try getInfo(refresh: true) {
            print("Min channel size: \(freshInfo.options.minChannelSizeSat)")
            print("Max channel size: \(freshInfo.options.maxChannelSizeSat)")
        }
        
    } catch {
        print("Error: \(error)")
    }
}
```

### Android (Kotlin)
```kotlin
import com.synonym.bitkitcore.*

fun manageBlocktank() {
    try {
        // Initialize database
        initDb("/path/to/data")  // Creates /path/to/data/blocktank.db
        
        // Update Blocktank URL
        updateBlocktankUrl("https://api1.blocktank.to/api")
        
        // Get cached info
        getInfo(refresh = false)?.let { info ->
            println("Current LSP nodes: ${info.nodes.size}")
            println("Network: ${info.onchain.network}")
            println("Fee rates: Fast ${info.onchain.feeRates.fast}")
        }
        
        // Fetch fresh info
        getInfo(refresh = true)?.let { freshInfo ->
            println("Min channel size: ${freshInfo.options.minChannelSizeSat}")
            println("Max channel size: ${freshInfo.options.maxChannelSizeSat}")
        }
        
    } catch (e: Exception) {
        println("Error: $e")
    }
}
```

### Python
```python
from bitkitcore import *

try:
    # Initialize database
    init_db("/path/to/data")  # Creates /path/to/data/blocktank.db
    
    # Update Blocktank URL
    update_blocktank_url("https://api1.blocktank.to/api")
    
    # Get cached info
    if info := get_info(refresh=False):
        print(f"Current LSP nodes: {len(info.nodes)}")
        print(f"Network: {info.onchain.network}")
        print(f"Fee rates: Fast {info.onchain.fee_rates.fast}")
    
    # Get fresh info
    if fresh_info := get_info(refresh=True):
        print(f"Min channel size: {fresh_info.options.min_channel_size_sat}")
        print(f"Max channel size: {fresh_info.options.max_channel_size_sat}")
    
except Exception as e:
    print(f"Error: {e}")
```