# Activity Module

The Activity module is responsible for storing and managing transaction/activity history for both Bitcoin and Lightning Network payments.

## Features
- Activity Tracking
  - Bitcoin & Lightning Network transactions
    - [`OnchainActivity`](#onchainactivity-fields): On-chain Bitcoin transactions
    - [`LightningActivity`](#lightningactivity-fields): Lightning Network transactions
- Tags
  - Add or remove tags from activities and filter activities by tags.

## Usage Examples

### iOS (Swift)
```swift
import BitkitCore

func manageActivities() {
    do {
        // Initialize with default filename
        initDb("/path/to/data")  // Creates /path/to/data/activity.db
        
        // Or with custom filename
        initDb("/path/to/data/custom.db")
        
        // Create and store an onchain activity
        let onchainActivity = OnchainActivity(
            id: "tx123",
            activityType: .onchain,
            txType: .sent,
            txId: "abc123",
            value: 50000,
            fee: 500,
            feeRate: 1,
            address: "bc1q...",
            confirmed: true,
            timestamp: 1234567890,
            isBoosted: false,
            isTransfer: false,
            exists: true,
            confirmTimestamp: 1234568890,
            channelId: nil,
            transferTxId: nil
        )
        
        // Wrap in Activity enum and insert
        let activity = Activity.onchain(onchainActivity)
        try insertActivity(activity: activity)
        
        // Retrieve activities with optional limit
        let allActivities = try getAllActivities(limit: 10)
        let onchainActivities = try getAllOnchainActivities(limit: 10)
        let lightningActivities = try getAllLightningActivities(limit: 10)
        
        // Get specific activity
        if let foundActivity = try getActivityById(activityId: "tx123") {
            switch foundActivity {
            case .onchain(let onchain):
                print("Found onchain activity: \(onchain.txId)")
            case .lightning(let lightning):
                print("Found lightning activity: \(lightning.preimage ?? "")")
            }
        }
        
        // Update activity
        let updatedActivity = Activity.onchain(onchainActivity)
        try updateActivity(activityId: "tx123", activity: updatedActivity)
        
        // Tag operations
        try addTags(activityId: "tx123", tags: ["payment", "coffee"])
        let tags = try getTags(activityId: "tx123")
        let taggedActivities = try getActivitiesByTag(tag: "coffee", limit: 5)
        
        try removeTags(activityId: "tx123", tags: ["payment"])
        
        // Delete activity
        let deleted = try deleteActivityById(activityId: "tx123")
        
    } catch {
        print("Error: \(error)")
    }
}
```

### Android (Kotlin)
```kotlin
import com.synonym.bitkitcore.*

fun manageActivities() {
    try {
        // Initialize with default filename
        initDb("/path/to/data")  // Creates /path/to/data/activity.db
        
        // Or with custom filename
        initDb("/path/to/data/custom.db")
        
        // Create and store a lightning activity
        val lightningActivity = LightningActivity(
            id = "ln456",
            activityType = ActivityType.LIGHTNING,
            txType = PaymentType.RECEIVED,
            status = PaymentState.SUCCEEDED,
            value = 10000,
            fee = 1,
            address = "example@lightning.addr",
            confirmed = true,
            message = "Payment for coffee",
            timestamp = 1234567890,
            preimage = "def456"
        )

        // Wrap in Activity enum and insert
        val activity = Activity.Lightning(lightningActivity)
        insertActivity(activity)
        
        // Retrieve activities with optional limit
        val allActivities = getAllActivities(limit = 20)
        val onchainActivities = getAllOnchainActivities(limit = 20)
        val lightningActivities = getAllLightningActivities(limit = 20)
        
        // Get specific activity
        getActivityById("ln456")?.let { foundActivity ->
            when (foundActivity) {
                is Activity.Onchain -> println("Found onchain activity: ${foundActivity.txId}")
                is Activity.Lightning -> println("Found lightning activity: ${foundActivity.preimage}")
            }
        }
        
        // Update activity
        val updatedActivity = Activity.Lightning(lightningActivity)
        updateActivity(activityId = "ln456", activity = updatedActivity)
        
        // Tag operations
        addTags(activityId = "ln456", tags = listOf("income", "coffee"))
        val tags = getTags(activityId = "ln456")
        val taggedActivities = getActivitiesByTag(tag = "coffee", limit = 5)

        removeTags(activityId = "ln456", tags = listOf("income"))
        
        // Delete activity
        val deleted = deleteActivityById(activityId = "ln456")
        
    } catch (e: Exception) {
        println("Error: $
    }
}
```

### Python
```python
from bitkitcore import *

try:
    # Initialize with default filename
    init_activity_db("/path/to/data")  # Creates /path/to/data/activity.db
    
    # Or with custom filename
    init_activity_db("/path/to/data/custom.db")
    
    # Create and store an onchain activity
    onchain_activity = OnchainActivity(
        id="tx123",
        activity_type=ActivityType.ONCHAIN,
        tx_type=PaymentType.SENT,
        tx_id="abc123",
        value=50000,
        fee=500,
        fee_rate=1,
        address="bc1q...",
        confirmed=True,
        timestamp=1234567890,
        is_boosted=False,
        is_transfer=False,
        exists=True,
        confirm_timestamp=1234568890,
        channel_id=None,
        transfer_tx_id=None
    )

    # Wrap in Activity enum and insert
    activity = Activity.Onchain(onchain_activity)
    insert_activity(activity)
    
    # Retrieve activities with optional limit
    all_activities = get_all_activities(limit=10)
    onchain_activities = get_all_onchain_activities(limit=10)
    lightning_activities = get_all_lightning_activities(limit=10)
    
    # Get specific activity
    if found_activity := get_activity_by_id("tx123"):
        if isinstance(found_activity, Activity.Onchain):
            print(f"Found onchain activity: {found_activity.tx_id}")
        elif isinstance(found_activity, Activity.Lightning):
            print(f"Found lightning activity: {found_activity.preimage}")
            
    # Update activity
    updated_activity = Activity.Onchain(onchain_activity)
    update_activity(activity_id="tx123", activity=updated_activity)
    
    # Tag operations
    add_tags("tx123", ["payment", "coffee"])
    tags = get_tags("tx123")
    tagged_activities = get_activities_by_tag("coffee", limit=5)
    
    remove_tags("tx123", ["payment"])

    # Delete activity
    deleted = delete_activity_by_id("tx123")
    
except Exception as e:
    print(f"Error: {e}")
```

## Supported Types

### ActivityType:
- `Onchain`: On-chain Bitcoin transactions
- `Lightning`: Lightning Network transactions
  
### PaymentType
- `Sent`: Outgoing payments
- `Received`: Incoming payments

### PaymentState
- `Pending`: Payment is in progress
- `Succeeded`: Payment completed successfully
- `Failed`: Payment failed

### OnchainActivity Fields
- `id`: String - Unique identifier
- `activity_type`: ActivityType - Type of activity (Onchain)
- `tx_type`: PaymentType - Type of transaction (Sent/Received)
- `tx_id`: String - Transaction ID
- `value`: i64 - Amount in satoshis
- `fee`: i64 - Transaction fee in satoshis
- `fee_rate`: i64 - Fee rate in sat/vB
- `address`: String - Bitcoin address
- `confirmed`: bool - Confirmation status
- `timestamp`: i64 - Transaction timestamp in seconds since epoch
- `is_boosted`: bool - RBF status
- `is_transfer`: bool - Internal transfer flag
- `does_exist`: bool - Transaction existence flag
- `confirm_timestamp`: Option<i64> - Confirmation timestamp (optional)
- `channel_id`: Option<String> - Associated channel ID (optional)
- `transfer_tx_id`: Option<String> - Related transfer transaction ID (optional)
- `created_at`: Option<i64> - Creation timestamp (optional)
- `updated_at`: Option<i64> - Last update timestamp (optional)

### LightningActivity Fields
- `id`: String - Unique identifier
- `activity_type`: ActivityType - Type of activity (Lightning)
- `tx_type`: PaymentType - Type of transaction (Sent/Received)
- `status`: PaymentState - Payment state (Pending/Succeeded/Failed)
- `value`: i64 - Amount in satoshis
- `fee`: Option<i64> - Payment fee in satoshis (optional)
- `invoice`: String - Lightning invoice
- `message`: String - Payment message
- `timestamp`: i64 - Transaction timestamp in seconds since epoch
- `preimage`: Option<String> - Payment preimage (optional)
- `created_at`: Option<i64> - Creation timestamp (optional)
- `updated_at`: Option<i64> - Last update timestamp (optional)

## Error Handling

Database operations can throw the following errors:
- Database initialization errors
- Insert operation failures
- Query execution failures
- Invalid data format errors
- Database connection errors