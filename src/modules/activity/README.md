# Activity Module

The Activity module is responsible for storing and managing transaction/activity history for both Bitcoin and Lightning Network payments.

## Features
- Activity Tracking
  - Bitcoin & Lightning Network transactions
    - [`OnchainActivity`](#onchainactivity-fields): On-chain Bitcoin transactions
    - [`LightningActivity`](#lightningactivity-fields): Lightning Network transactions
- Tags
  - Add or remove tags from activities and filter activities by tags.

## Available Methods

```rust
// Initialize the database with a specified path
fn init_db(base_path: String) -> Result<String, DbError>

// Get activities with optional filter, tx_type, tags, search, min_date, max_date, limit, and sort direction
fn get_activities(
  filter: Option<ActivityFilter>,
  tx_type: Option<PaymentType>,
  tags: Option<Vec<String>>,
  search: Option<String>,
  min_date: Option<u64>,
  max_date: Option<u64>,
  limit: Option<u32>,
  sort_direction: Option<SortDirection>
) -> Result<Vec<Activity>, ActivityError>

// Get activities by tag with optional limit and sort direction
fn get_activities_by_tag(
  tag: String,
  limit: Option<u32>,
  sort_direction: Option<SortDirection>
) -> Result<Vec<Activity>, ActivityError>

// Insert a new activity
fn insert_activity(activity: Activity) -> Result<(), ActivityError>

// Update an existing activity
fn update_activity(activity_id: String, activity: Activity) -> Result<(), ActivityError>

// Insert or update an activity
fn upsert_activity(activity: Activity) -> Result<(), ActivityError>

// Get a specific activity by ID
fn get_activity_by_id(activity_id: String) -> Result<Option<Activity>, ActivityError>

// Delete an activity by ID
fn delete_activity_by_id(activity_id: String) -> Result<bool, ActivityError>

// Tag management
fn add_tags(activity_id: String, tags: Vec<String>) -> Result<(), ActivityError>
fn remove_tags(activity_id: String, tags: Vec<String>) -> Result<(), ActivityError>
fn get_tags(activity_id: String) -> Result<Vec<String>, ActivityError>
fn get_all_unique_tags() -> Result<Vec<String>, ActivityError>
```

## Usage Examples

### iOS (Swift)
```swift
import BitkitCore

func manageActivities() {
    do {
        // Initialize database
        try initDb("/path/to/data")  // Creates /path/to/data/activity.db
        
        // Create and store an onchain activity
        let onchainActivity = OnchainActivity(
            id: "tx123",
            tx_type: .sent,
            tx_id: "abc123",
            value: 50000,
            fee: 500,
            fee_rate: 1,
            address: "bc1q...",
            confirmed: true,
            timestamp: 1234567890,
            is_boosted: false,
            is_transfer: false,
            does_exist: true,
            confirm_timestamp: 1234568890,
            channel_id: nil,
            transfer_tx_id: nil
        )
        
        // Wrap in Activity enum and insert
        let activity = Activity.onchain(onchainActivity)
        try insertActivity(activity: activity)
        
        // Retrieve activities with advanced filtering
        let filteredActivities = try getActivities(
            filter: .all,
            txType: .sent,
            tags: ["coffee", "food"],
            search: "bc1q",
            minDate: 1234567890,
            maxDate: 1234667890,
            limit: 10,
            sortDirection: .desc
        )
        
        // Simple query (all parameters are optional)
        let simpleQuery = try getActivities(
            filter: .all,
            txType: nil,
            tags: nil,
            search: nil,
            minDate: nil,
            maxDate: nil,
            limit: 10,
            sortDirection: .desc
        )
        
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
        let taggedActivities = try getActivitiesByTag(tag: "coffee", limit: 5, sortDirection: .desc)
        
        // Get all unique tags
        let allUniqueTags = try getAllUniqueTags()  // ["coffee", "food", "payment"]
        
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
        // Initialize database
        initDb("/path/to/data")  // Creates /path/to/data/activity.db
        
        // Create and store a lightning activity
        val lightningActivity = LightningActivity(
            id = "ln456",
            tx_type = PaymentType.RECEIVED,
            status = PaymentState.SUCCEEDED,
            value = 10000,
            fee = 1,
            invoice = "lnbc...",
            message = "Payment for coffee",
            timestamp = 1234567890,
            preimage = "def456"
        )

        // Wrap in Activity enum and insert
        val activity = Activity.Lightning(lightningActivity)
        insertActivity(activity)
        
        // Retrieve activities with advanced filtering
        val filteredActivities = getActivities(
            filter = ActivityFilter.ALL,
            txType = PaymentType.SENT,
            tags = listOf("coffee", "food"),
            search = "bc1q",
            minDate = 1234567890,
            maxDate = 1234667890,
            limit = 20,
            sortDirection = SortDirection.DESC
        )
        
        // Simple query (all parameters are optional)
        val simpleQuery = getActivities(
            filter = ActivityFilter.ALL,
            txType = null,
            tags = null,
            search = null,
            minDate = null,
            maxDate = null,
            limit = 20,
            sortDirection = SortDirection.DESC
        )
        
        // Filter by specific criteria
        val sentPayments = getActivities(
            filter = ActivityFilter.ALL,
            txType = PaymentType.SENT,
            limit = 20
        )
        
        val recentLightning = getActivities(
            filter = ActivityFilter.LIGHTNING,
            minDate = System.currentTimeMillis() / 1000 - 86400, // Last 24 hours
            limit = 20
        )
        
        val taggedPayments = getActivities(
            filter = ActivityFilter.ALL,
            tags = listOf("coffee"),
            limit = 20
        )
        
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
        val taggedActivities = getActivitiesByTag(
            tag = "coffee",
            limit = 5,
            sortDirection = SortDirection.DESC
        )
        
        // Get all unique tags
        val allUniqueTags = getAllUniqueTags()  // ["coffee", "food", "payment"]

        removeTags(activityId = "ln456", tags = listOf("income"))
        
        // Delete activity
        val deleted = deleteActivityById(activityId = "ln456")
        
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
    init_db("/path/to/data")  # Creates /path/to/data/activity.db
    
    # Create and store an onchain activity
    onchain_activity = OnchainActivity(
        id="tx123",
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
        does_exist=True,
        confirm_timestamp=1234568890,
        channel_id=None,
        transfer_tx_id=None
    )

    # Wrap in Activity enum and insert
    activity = Activity.Onchain(onchain_activity)
    insert_activity(activity)
    
    # Retrieve activities with advanced filtering
    filtered_activities = get_activities(
        filter=ActivityFilter.ALL,
        tx_type=PaymentType.SENT,
        tags=["coffee", "food"],
        search="bc1q",
        min_date=1234567890,
        max_date=1234667890,
        limit=10,
        sort_direction=SortDirection.DESC
    )
    
    # Simple query (all parameters are optional)
    simple_query = get_activities(
        filter=ActivityFilter.ALL,
        tx_type=None,
        tags=None,
        search=None,
        min_date=None,
        max_date=None,
        limit=10,
        sort_direction=SortDirection.DESC
    )
    
    # Filter by specific criteria
    sent_payments = get_activities(
        filter=ActivityFilter.ALL,
        tx_type=PaymentType.SENT,
        limit=10
    )
    
    recent_lightning = get_activities(
        filter=ActivityFilter.LIGHTNING,
        min_date=int(time.time()) - 86400,  # Last 24 hours
        limit=10
    )
    
    tagged_payments = get_activities(
        filter=ActivityFilter.ALL,
        tags=["coffee"],
        limit=10
    )
    
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
    tagged_activities = get_activities_by_tag(
        tag="coffee",
        limit=5,
        sort_direction=SortDirection.DESC
    )
    
    # Get all unique tags with optional sorting
    all_unique_tags = get_all_unique_tags()  # ["coffee", "food", "payment"]
    
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
- `tx_type`: PaymentType - Type of transaction (Sent/Received)
- `tx_id`: String - Transaction ID
- `value`: u64 - Amount in satoshis
- `fee`: u64 - Transaction fee in satoshis
- `fee_rate`: u64 - Fee rate in sat/vB
- `address`: String - Bitcoin address
- `confirmed`: bool - Confirmation status
- `timestamp`: u64 - Transaction timestamp in seconds since epoch
- `is_boosted`: bool - RBF status
- `is_transfer`: bool - Internal transfer flag
- `does_exist`: bool - Transaction existence flag
- `confirm_timestamp`: Option<u64> - Confirmation timestamp (optional)
- `channel_id`: Option<String> - Associated channel ID (optional)
- `transfer_tx_id`: Option<String> - Related transfer transaction ID (optional)
- `created_at`: Option<u64> - Creation timestamp (optional)
- `updated_at`: Option<u64> - Last update timestamp (optional)

### LightningActivity Fields
- `id`: String - Unique identifier
- `tx_type`: PaymentType - Type of transaction (Sent/Received)
- `status`: PaymentState - Payment state (Pending/Succeeded/Failed)
- `value`: u64 - Amount in satoshis
- `fee`: Option<u64> - Payment fee in satoshis (optional)
- `invoice`: String - Lightning invoice
- `message`: String - Payment message
- `timestamp`: u64 - Transaction timestamp in seconds since epoch
- `preimage`: Option<String> - Payment preimage (optional)
- `created_at`: Option<u64> - Creation timestamp (optional)
- `updated_at`: Option<u64> - Last update timestamp (optional)

## Activity Types and Data Structures

### Activity Filters
```rust
pub enum ActivityFilter {
    All,        // Get all activities
    Lightning,  // Get only lightning activities
    Onchain,    // Get only onchain activities
}
```
```rust
pub enum SortDirection {
    Asc,     // Sort in ascending order
    Desc,    // Sort in descending order
}
```
Note: When no sort direction is specified (sort_direction = None), activities are returned in
descending order (newest first) by default.

## Error Handling

The module uses the `ActivityError` enum which includes:
- `InitializationError`: Database setup and initialization failures
- `ConnectionError`: Database connection issues
- `DataError`: Issues with data format or constraints
- `InsertError`: Failures during insert operations
- `RetrievalError`: Failures during data retrieval
- `UpdateError`: Failures during update operations