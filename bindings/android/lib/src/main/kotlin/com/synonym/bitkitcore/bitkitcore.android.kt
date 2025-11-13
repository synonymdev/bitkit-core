

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

import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Structure
import kotlin.coroutines.resume
import kotlinx.coroutines.CancellableContinuation
import kotlinx.coroutines.DelicateCoroutinesApi
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.GlobalScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext


internal typealias Pointer = com.sun.jna.Pointer
internal val NullPointer: Pointer? = com.sun.jna.Pointer.NULL
internal fun Pointer.toLong(): Long = Pointer.nativeValue(this)
internal fun kotlin.Long.toPointer() = com.sun.jna.Pointer(this)


@kotlin.jvm.JvmInline
public value class ByteBuffer(private val inner: java.nio.ByteBuffer) {
    init {
        inner.order(java.nio.ByteOrder.BIG_ENDIAN)
    }

    public fun internal(): java.nio.ByteBuffer = inner

    public fun limit(): Int = inner.limit()

    public fun position(): Int = inner.position()

    public fun hasRemaining(): Boolean = inner.hasRemaining()

    public fun get(): Byte = inner.get()

    public fun get(bytesToRead: Int): ByteArray = ByteArray(bytesToRead).apply(inner::get)

    public fun getShort(): Short = inner.getShort()

    public fun getInt(): Int = inner.getInt()

    public fun getLong(): Long = inner.getLong()

    public fun getFloat(): Float = inner.getFloat()

    public fun getDouble(): Double = inner.getDouble()

    public fun put(value: Byte) {
        inner.put(value)
    }

    public fun put(src: ByteArray) {
        inner.put(src)
    }

    public fun putShort(value: Short) {
        inner.putShort(value)
    }

    public fun putInt(value: Int) {
        inner.putInt(value)
    }

    public fun putLong(value: Long) {
        inner.putLong(value)
    }

    public fun putFloat(value: Float) {
        inner.putFloat(value)
    }

    public fun putDouble(value: Double) {
        inner.putDouble(value)
    }
}
public fun RustBuffer.setValue(array: RustBufferByValue) {
    this.data = array.data
    this.len = array.len
    this.capacity = array.capacity
}

internal object RustBufferHelper {
    internal fun allocValue(size: ULong = 0UL): RustBufferByValue = uniffiRustCall { status ->
        // Note: need to convert the size to a `Long` value to make this work with JVM.
        UniffiLib.ffi_bitkitcore_rustbuffer_alloc(size.toLong(), status)
    }.also {
        if(it.data == null) {
            throw RuntimeException("RustBuffer.alloc() returned null data pointer (size=${size})")
        }
    }

    internal fun free(buf: RustBufferByValue) = uniffiRustCall { status ->
        UniffiLib.ffi_bitkitcore_rustbuffer_free(buf, status)
    }
}

@Structure.FieldOrder("capacity", "len", "data")
public open class RustBufferStruct(
    // Note: `capacity` and `len` are actually `ULong` values, but JVM only supports signed values.
    // When dealing with these fields, make sure to call `toULong()`.
    @JvmField public var capacity: Long,
    @JvmField public var len: Long,
    @JvmField public var data: Pointer?,
) : Structure() {
    public constructor(): this(0.toLong(), 0.toLong(), null)

    public class ByValue(
        capacity: Long,
        len: Long,
        data: Pointer?,
    ): RustBuffer(capacity, len, data), Structure.ByValue {
        public constructor(): this(0.toLong(), 0.toLong(), null)
    }

    /**
     * The equivalent of the `*mut RustBuffer` type.
     * Required for callbacks taking in an out pointer.
     *
     * Size is the sum of all values in the struct.
     */
    public class ByReference(
        capacity: Long,
        len: Long,
        data: Pointer?,
    ): RustBuffer(capacity, len, data), Structure.ByReference {
        public constructor(): this(0.toLong(), 0.toLong(), null)
    }
}

public typealias RustBuffer = RustBufferStruct
public typealias RustBufferByValue = RustBufferStruct.ByValue

internal fun RustBuffer.asByteBuffer(): ByteBuffer? {
    require(this.len <= Int.MAX_VALUE) {
        val length = this.len
        "cannot handle RustBuffer longer than Int.MAX_VALUE bytes: length is $length"
    }
    return ByteBuffer(data?.getByteBuffer(0L, this.len) ?: return null)
}

internal fun RustBufferByValue.asByteBuffer(): ByteBuffer? {
    require(this.len <= Int.MAX_VALUE) {
        val length = this.len
        "cannot handle RustBuffer longer than Int.MAX_VALUE bytes: length is $length"
    }
    return ByteBuffer(data?.getByteBuffer(0L, this.len) ?: return null)
}

// This is a helper for safely passing byte references into the rust code.
// It's not actually used at the moment, because there aren't many things that you
// can take a direct pointer to in the JVM, and if we're going to copy something
// then we might as well copy it into a `RustBuffer`. But it's here for API
// completeness.

@Structure.FieldOrder("len", "data")
internal open class ForeignBytesStruct : Structure() {
    @JvmField var len: Int = 0
    @JvmField var data: Pointer? = null

    internal class ByValue : ForeignBytes(), Structure.ByValue
}
internal typealias ForeignBytes = ForeignBytesStruct
internal typealias ForeignBytesByValue = ForeignBytesStruct.ByValue

public interface FfiConverter<KotlinType, FfiType> {
    // Convert an FFI type to a Kotlin type
    public fun lift(value: FfiType): KotlinType

    // Convert an Kotlin type to an FFI type
    public fun lower(value: KotlinType): FfiType

    // Read a Kotlin type from a `ByteBuffer`
    public fun read(buf: ByteBuffer): KotlinType

    // Calculate bytes to allocate when creating a `RustBuffer`
    //
    // This must return at least as many bytes as the write() function will
    // write. It can return more bytes than needed, for example when writing
    // Strings we can't know the exact bytes needed until we the UTF-8
    // encoding, so we pessimistically allocate the largest size possible (3
    // bytes per codepoint).  Allocating extra bytes is not really a big deal
    // because the `RustBuffer` is short-lived.
    public fun allocationSize(value: KotlinType): ULong

    // Write a Kotlin type to a `ByteBuffer`
    public fun write(value: KotlinType, buf: ByteBuffer)

    // Lower a value into a `RustBuffer`
    //
    // This method lowers a value into a `RustBuffer` rather than the normal
    // FfiType.  It's used by the callback interface code.  Callback interface
    // returns are always serialized into a `RustBuffer` regardless of their
    // normal FFI type.
    public fun lowerIntoRustBuffer(value: KotlinType): RustBufferByValue {
        val rbuf = RustBufferHelper.allocValue(allocationSize(value))
        val bbuf = rbuf.asByteBuffer()!!
        write(value, bbuf)
        return RustBufferByValue(
            capacity = rbuf.capacity,
            len = bbuf.position().toLong(),
            data = rbuf.data,
        )
    }

    // Lift a value from a `RustBuffer`.
    //
    // This here mostly because of the symmetry with `lowerIntoRustBuffer()`.
    // It's currently only used by the `FfiConverterRustBuffer` class below.
    public fun liftFromRustBuffer(rbuf: RustBufferByValue): KotlinType {
        val byteBuf = rbuf.asByteBuffer()!!
        try {
           val item = read(byteBuf)
           if (byteBuf.hasRemaining()) {
               throw RuntimeException("junk remaining in buffer after lifting, something is very wrong!!")
           }
           return item
        } finally {
            RustBufferHelper.free(rbuf)
        }
    }
}

// FfiConverter that uses `RustBuffer` as the FfiType
public interface FfiConverterRustBuffer<KotlinType>: FfiConverter<KotlinType, RustBufferByValue> {
    override fun lift(value: RustBufferByValue): KotlinType = liftFromRustBuffer(value)
    override fun lower(value: KotlinType): RustBufferByValue = lowerIntoRustBuffer(value)
}

internal const val UNIFFI_CALL_SUCCESS = 0.toByte()
internal const val UNIFFI_CALL_ERROR = 1.toByte()
internal const val UNIFFI_CALL_UNEXPECTED_ERROR = 2.toByte()

// Default Implementations
internal fun UniffiRustCallStatus.isSuccess(): Boolean
    = code == UNIFFI_CALL_SUCCESS

internal fun UniffiRustCallStatus.isError(): Boolean
    = code == UNIFFI_CALL_ERROR

internal fun UniffiRustCallStatus.isPanic(): Boolean
    = code == UNIFFI_CALL_UNEXPECTED_ERROR

internal fun UniffiRustCallStatusByValue.isSuccess(): Boolean
    = code == UNIFFI_CALL_SUCCESS

internal fun UniffiRustCallStatusByValue.isError(): Boolean
    = code == UNIFFI_CALL_ERROR

internal fun UniffiRustCallStatusByValue.isPanic(): Boolean
    = code == UNIFFI_CALL_UNEXPECTED_ERROR

// Each top-level error class has a companion object that can lift the error from the call status's rust buffer
public interface UniffiRustCallStatusErrorHandler<E> {
    public fun lift(errorBuf: RustBufferByValue): E
}

// Helpers for calling Rust
// In practice we usually need to be synchronized to call this safely, so it doesn't
// synchronize itself

// Call a rust function that returns a Result<>.  Pass in the Error class companion that corresponds to the Err
internal inline fun <U, E: kotlin.Exception> uniffiRustCallWithError(errorHandler: UniffiRustCallStatusErrorHandler<E>, crossinline callback: (UniffiRustCallStatus) -> U): U {
    return UniffiRustCallStatusHelper.withReference() { status ->
        val returnValue = callback(status)
        uniffiCheckCallStatus(errorHandler, status)
        returnValue
    }
}

// Check `status` and throw an error if the call wasn't successful
internal fun<E: kotlin.Exception> uniffiCheckCallStatus(errorHandler: UniffiRustCallStatusErrorHandler<E>, status: UniffiRustCallStatus) {
    if (status.isSuccess()) {
        return
    } else if (status.isError()) {
        throw errorHandler.lift(status.errorBuf)
    } else if (status.isPanic()) {
        // when the rust code sees a panic, it tries to construct a rustbuffer
        // with the message.  but if that code panics, then it just sends back
        // an empty buffer.
        if (status.errorBuf.len > 0) {
            throw InternalException(FfiConverterString.lift(status.errorBuf))
        } else {
            throw InternalException("Rust panic")
        }
    } else {
        throw InternalException("Unknown rust call status: $status.code")
    }
}

// UniffiRustCallStatusErrorHandler implementation for times when we don't expect a CALL_ERROR
public object UniffiNullRustCallStatusErrorHandler: UniffiRustCallStatusErrorHandler<InternalException> {
    override fun lift(errorBuf: RustBufferByValue): InternalException {
        RustBufferHelper.free(errorBuf)
        return InternalException("Unexpected CALL_ERROR")
    }
}

// Call a rust function that returns a plain value
internal inline fun <U> uniffiRustCall(crossinline callback: (UniffiRustCallStatus) -> U): U {
    return uniffiRustCallWithError(UniffiNullRustCallStatusErrorHandler, callback)
}

internal inline fun<T> uniffiTraitInterfaceCall(
    callStatus: UniffiRustCallStatus,
    makeCall: () -> T,
    writeReturn: (T) -> Unit,
) {
    try {
        writeReturn(makeCall())
    } catch(e: kotlin.Exception) {
        callStatus.code = UNIFFI_CALL_UNEXPECTED_ERROR
        callStatus.errorBuf = FfiConverterString.lower(e.toString())
    }
}

internal inline fun<T, reified E: Throwable> uniffiTraitInterfaceCallWithError(
    callStatus: UniffiRustCallStatus,
    makeCall: () -> T,
    writeReturn: (T) -> Unit,
    lowerError: (E) -> RustBufferByValue
) {
    try {
        writeReturn(makeCall())
    } catch(e: kotlin.Exception) {
        if (e is E) {
            callStatus.code = UNIFFI_CALL_ERROR
            callStatus.errorBuf = lowerError(e)
        } else {
            callStatus.code = UNIFFI_CALL_UNEXPECTED_ERROR
            callStatus.errorBuf = FfiConverterString.lower(e.toString())
        }
    }
}

@Structure.FieldOrder("code", "errorBuf")
internal open class UniffiRustCallStatusStruct(
    @JvmField public var code: Byte,
    @JvmField public var errorBuf: RustBufferByValue,
) : Structure() {
    internal constructor(): this(0.toByte(), RustBufferByValue())

    internal class ByValue(
        code: Byte,
        errorBuf: RustBufferByValue,
    ): UniffiRustCallStatusStruct(code, errorBuf), Structure.ByValue {
        internal constructor(): this(0.toByte(), RustBufferByValue())
    }
    internal class ByReference(
        code: Byte,
        errorBuf: RustBufferByValue,
    ): UniffiRustCallStatusStruct(code, errorBuf), Structure.ByReference {
        internal constructor(): this(0.toByte(), RustBufferByValue())
    }
}

internal typealias UniffiRustCallStatus = UniffiRustCallStatusStruct.ByReference
internal typealias UniffiRustCallStatusByValue = UniffiRustCallStatusStruct.ByValue

internal object UniffiRustCallStatusHelper {
    internal fun allocValue() = UniffiRustCallStatusByValue()
    internal fun <U> withReference(block: (UniffiRustCallStatus) -> U): U {
        val status = UniffiRustCallStatus()
        return block(status)
    }
}

internal class UniffiHandleMap<T: Any> {
    private val map = java.util.concurrent.ConcurrentHashMap<Long, T>()
    private val counter: kotlinx.atomicfu.AtomicLong = kotlinx.atomicfu.atomic(1L)

    internal val size: Int
        get() = map.size

    // Insert a new object into the handle map and get a handle for it
    internal fun insert(obj: T): Long {
        val handle = counter.getAndAdd(1)
        map[handle] = obj
        return handle
    }

    // Get an object from the handle map
    internal fun get(handle: Long): T {
        return map[handle] ?: throw InternalException("UniffiHandleMap.get: Invalid handle")
    }

    // Remove an entry from the handlemap and get the Kotlin object back
    internal fun remove(handle: Long): T {
        return map.remove(handle) ?: throw InternalException("UniffiHandleMap.remove: Invalid handle")
    }
}

internal typealias ByteByReference = com.sun.jna.ptr.ByteByReference
internal typealias DoubleByReference = com.sun.jna.ptr.DoubleByReference
internal typealias FloatByReference = com.sun.jna.ptr.FloatByReference
internal typealias IntByReference = com.sun.jna.ptr.IntByReference
internal typealias LongByReference = com.sun.jna.ptr.LongByReference
internal typealias PointerByReference = com.sun.jna.ptr.PointerByReference
internal typealias ShortByReference = com.sun.jna.ptr.ShortByReference

// Contains loading, initialization code,
// and the FFI Function declarations in a com.sun.jna.Library.

// Define FFI callback types
internal interface UniffiRustFutureContinuationCallback: com.sun.jna.Callback {
    public fun callback(`data`: Long,`pollResult`: Byte,)
}
internal interface UniffiForeignFutureFree: com.sun.jna.Callback {
    public fun callback(`handle`: Long,)
}
internal interface UniffiCallbackInterfaceFree: com.sun.jna.Callback {
    public fun callback(`handle`: Long,)
}
@Structure.FieldOrder("handle", "free")
internal open class UniffiForeignFutureStruct(
    @JvmField public var `handle`: Long,
    @JvmField public var `free`: UniffiForeignFutureFree?,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `handle` = 0.toLong(),
        
        `free` = null,
        
    )

    internal class UniffiByValue(
        `handle`: Long,
        `free`: UniffiForeignFutureFree?,
    ): UniffiForeignFuture(`handle`,`free`,), Structure.ByValue
}

internal typealias UniffiForeignFuture = UniffiForeignFutureStruct

internal fun UniffiForeignFuture.uniffiSetValue(other: UniffiForeignFuture) {
    `handle` = other.`handle`
    `free` = other.`free`
}
internal fun UniffiForeignFuture.uniffiSetValue(other: UniffiForeignFutureUniffiByValue) {
    `handle` = other.`handle`
    `free` = other.`free`
}

internal typealias UniffiForeignFutureUniffiByValue = UniffiForeignFutureStruct.UniffiByValue
@Structure.FieldOrder("returnValue", "callStatus")
internal open class UniffiForeignFutureStructU8Struct(
    @JvmField public var `returnValue`: Byte,
    @JvmField public var `callStatus`: UniffiRustCallStatusByValue,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `returnValue` = 0.toByte(),
        
        `callStatus` = UniffiRustCallStatusHelper.allocValue(),
        
    )

    internal class UniffiByValue(
        `returnValue`: Byte,
        `callStatus`: UniffiRustCallStatusByValue,
    ): UniffiForeignFutureStructU8(`returnValue`,`callStatus`,), Structure.ByValue
}

internal typealias UniffiForeignFutureStructU8 = UniffiForeignFutureStructU8Struct

internal fun UniffiForeignFutureStructU8.uniffiSetValue(other: UniffiForeignFutureStructU8) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}
internal fun UniffiForeignFutureStructU8.uniffiSetValue(other: UniffiForeignFutureStructU8UniffiByValue) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}

internal typealias UniffiForeignFutureStructU8UniffiByValue = UniffiForeignFutureStructU8Struct.UniffiByValue
internal interface UniffiForeignFutureCompleteU8: com.sun.jna.Callback {
    public fun callback(`callbackData`: Long,`result`: UniffiForeignFutureStructU8UniffiByValue,)
}
@Structure.FieldOrder("returnValue", "callStatus")
internal open class UniffiForeignFutureStructI8Struct(
    @JvmField public var `returnValue`: Byte,
    @JvmField public var `callStatus`: UniffiRustCallStatusByValue,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `returnValue` = 0.toByte(),
        
        `callStatus` = UniffiRustCallStatusHelper.allocValue(),
        
    )

    internal class UniffiByValue(
        `returnValue`: Byte,
        `callStatus`: UniffiRustCallStatusByValue,
    ): UniffiForeignFutureStructI8(`returnValue`,`callStatus`,), Structure.ByValue
}

internal typealias UniffiForeignFutureStructI8 = UniffiForeignFutureStructI8Struct

internal fun UniffiForeignFutureStructI8.uniffiSetValue(other: UniffiForeignFutureStructI8) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}
internal fun UniffiForeignFutureStructI8.uniffiSetValue(other: UniffiForeignFutureStructI8UniffiByValue) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}

internal typealias UniffiForeignFutureStructI8UniffiByValue = UniffiForeignFutureStructI8Struct.UniffiByValue
internal interface UniffiForeignFutureCompleteI8: com.sun.jna.Callback {
    public fun callback(`callbackData`: Long,`result`: UniffiForeignFutureStructI8UniffiByValue,)
}
@Structure.FieldOrder("returnValue", "callStatus")
internal open class UniffiForeignFutureStructU16Struct(
    @JvmField public var `returnValue`: Short,
    @JvmField public var `callStatus`: UniffiRustCallStatusByValue,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `returnValue` = 0.toShort(),
        
        `callStatus` = UniffiRustCallStatusHelper.allocValue(),
        
    )

    internal class UniffiByValue(
        `returnValue`: Short,
        `callStatus`: UniffiRustCallStatusByValue,
    ): UniffiForeignFutureStructU16(`returnValue`,`callStatus`,), Structure.ByValue
}

internal typealias UniffiForeignFutureStructU16 = UniffiForeignFutureStructU16Struct

internal fun UniffiForeignFutureStructU16.uniffiSetValue(other: UniffiForeignFutureStructU16) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}
internal fun UniffiForeignFutureStructU16.uniffiSetValue(other: UniffiForeignFutureStructU16UniffiByValue) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}

internal typealias UniffiForeignFutureStructU16UniffiByValue = UniffiForeignFutureStructU16Struct.UniffiByValue
internal interface UniffiForeignFutureCompleteU16: com.sun.jna.Callback {
    public fun callback(`callbackData`: Long,`result`: UniffiForeignFutureStructU16UniffiByValue,)
}
@Structure.FieldOrder("returnValue", "callStatus")
internal open class UniffiForeignFutureStructI16Struct(
    @JvmField public var `returnValue`: Short,
    @JvmField public var `callStatus`: UniffiRustCallStatusByValue,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `returnValue` = 0.toShort(),
        
        `callStatus` = UniffiRustCallStatusHelper.allocValue(),
        
    )

    internal class UniffiByValue(
        `returnValue`: Short,
        `callStatus`: UniffiRustCallStatusByValue,
    ): UniffiForeignFutureStructI16(`returnValue`,`callStatus`,), Structure.ByValue
}

internal typealias UniffiForeignFutureStructI16 = UniffiForeignFutureStructI16Struct

internal fun UniffiForeignFutureStructI16.uniffiSetValue(other: UniffiForeignFutureStructI16) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}
internal fun UniffiForeignFutureStructI16.uniffiSetValue(other: UniffiForeignFutureStructI16UniffiByValue) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}

internal typealias UniffiForeignFutureStructI16UniffiByValue = UniffiForeignFutureStructI16Struct.UniffiByValue
internal interface UniffiForeignFutureCompleteI16: com.sun.jna.Callback {
    public fun callback(`callbackData`: Long,`result`: UniffiForeignFutureStructI16UniffiByValue,)
}
@Structure.FieldOrder("returnValue", "callStatus")
internal open class UniffiForeignFutureStructU32Struct(
    @JvmField public var `returnValue`: Int,
    @JvmField public var `callStatus`: UniffiRustCallStatusByValue,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `returnValue` = 0,
        
        `callStatus` = UniffiRustCallStatusHelper.allocValue(),
        
    )

    internal class UniffiByValue(
        `returnValue`: Int,
        `callStatus`: UniffiRustCallStatusByValue,
    ): UniffiForeignFutureStructU32(`returnValue`,`callStatus`,), Structure.ByValue
}

internal typealias UniffiForeignFutureStructU32 = UniffiForeignFutureStructU32Struct

internal fun UniffiForeignFutureStructU32.uniffiSetValue(other: UniffiForeignFutureStructU32) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}
internal fun UniffiForeignFutureStructU32.uniffiSetValue(other: UniffiForeignFutureStructU32UniffiByValue) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}

internal typealias UniffiForeignFutureStructU32UniffiByValue = UniffiForeignFutureStructU32Struct.UniffiByValue
internal interface UniffiForeignFutureCompleteU32: com.sun.jna.Callback {
    public fun callback(`callbackData`: Long,`result`: UniffiForeignFutureStructU32UniffiByValue,)
}
@Structure.FieldOrder("returnValue", "callStatus")
internal open class UniffiForeignFutureStructI32Struct(
    @JvmField public var `returnValue`: Int,
    @JvmField public var `callStatus`: UniffiRustCallStatusByValue,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `returnValue` = 0,
        
        `callStatus` = UniffiRustCallStatusHelper.allocValue(),
        
    )

    internal class UniffiByValue(
        `returnValue`: Int,
        `callStatus`: UniffiRustCallStatusByValue,
    ): UniffiForeignFutureStructI32(`returnValue`,`callStatus`,), Structure.ByValue
}

internal typealias UniffiForeignFutureStructI32 = UniffiForeignFutureStructI32Struct

internal fun UniffiForeignFutureStructI32.uniffiSetValue(other: UniffiForeignFutureStructI32) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}
internal fun UniffiForeignFutureStructI32.uniffiSetValue(other: UniffiForeignFutureStructI32UniffiByValue) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}

internal typealias UniffiForeignFutureStructI32UniffiByValue = UniffiForeignFutureStructI32Struct.UniffiByValue
internal interface UniffiForeignFutureCompleteI32: com.sun.jna.Callback {
    public fun callback(`callbackData`: Long,`result`: UniffiForeignFutureStructI32UniffiByValue,)
}
@Structure.FieldOrder("returnValue", "callStatus")
internal open class UniffiForeignFutureStructU64Struct(
    @JvmField public var `returnValue`: Long,
    @JvmField public var `callStatus`: UniffiRustCallStatusByValue,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `returnValue` = 0.toLong(),
        
        `callStatus` = UniffiRustCallStatusHelper.allocValue(),
        
    )

    internal class UniffiByValue(
        `returnValue`: Long,
        `callStatus`: UniffiRustCallStatusByValue,
    ): UniffiForeignFutureStructU64(`returnValue`,`callStatus`,), Structure.ByValue
}

internal typealias UniffiForeignFutureStructU64 = UniffiForeignFutureStructU64Struct

internal fun UniffiForeignFutureStructU64.uniffiSetValue(other: UniffiForeignFutureStructU64) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}
internal fun UniffiForeignFutureStructU64.uniffiSetValue(other: UniffiForeignFutureStructU64UniffiByValue) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}

internal typealias UniffiForeignFutureStructU64UniffiByValue = UniffiForeignFutureStructU64Struct.UniffiByValue
internal interface UniffiForeignFutureCompleteU64: com.sun.jna.Callback {
    public fun callback(`callbackData`: Long,`result`: UniffiForeignFutureStructU64UniffiByValue,)
}
@Structure.FieldOrder("returnValue", "callStatus")
internal open class UniffiForeignFutureStructI64Struct(
    @JvmField public var `returnValue`: Long,
    @JvmField public var `callStatus`: UniffiRustCallStatusByValue,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `returnValue` = 0.toLong(),
        
        `callStatus` = UniffiRustCallStatusHelper.allocValue(),
        
    )

    internal class UniffiByValue(
        `returnValue`: Long,
        `callStatus`: UniffiRustCallStatusByValue,
    ): UniffiForeignFutureStructI64(`returnValue`,`callStatus`,), Structure.ByValue
}

internal typealias UniffiForeignFutureStructI64 = UniffiForeignFutureStructI64Struct

internal fun UniffiForeignFutureStructI64.uniffiSetValue(other: UniffiForeignFutureStructI64) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}
internal fun UniffiForeignFutureStructI64.uniffiSetValue(other: UniffiForeignFutureStructI64UniffiByValue) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}

internal typealias UniffiForeignFutureStructI64UniffiByValue = UniffiForeignFutureStructI64Struct.UniffiByValue
internal interface UniffiForeignFutureCompleteI64: com.sun.jna.Callback {
    public fun callback(`callbackData`: Long,`result`: UniffiForeignFutureStructI64UniffiByValue,)
}
@Structure.FieldOrder("returnValue", "callStatus")
internal open class UniffiForeignFutureStructF32Struct(
    @JvmField public var `returnValue`: Float,
    @JvmField public var `callStatus`: UniffiRustCallStatusByValue,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `returnValue` = 0.0f,
        
        `callStatus` = UniffiRustCallStatusHelper.allocValue(),
        
    )

    internal class UniffiByValue(
        `returnValue`: Float,
        `callStatus`: UniffiRustCallStatusByValue,
    ): UniffiForeignFutureStructF32(`returnValue`,`callStatus`,), Structure.ByValue
}

internal typealias UniffiForeignFutureStructF32 = UniffiForeignFutureStructF32Struct

internal fun UniffiForeignFutureStructF32.uniffiSetValue(other: UniffiForeignFutureStructF32) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}
internal fun UniffiForeignFutureStructF32.uniffiSetValue(other: UniffiForeignFutureStructF32UniffiByValue) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}

internal typealias UniffiForeignFutureStructF32UniffiByValue = UniffiForeignFutureStructF32Struct.UniffiByValue
internal interface UniffiForeignFutureCompleteF32: com.sun.jna.Callback {
    public fun callback(`callbackData`: Long,`result`: UniffiForeignFutureStructF32UniffiByValue,)
}
@Structure.FieldOrder("returnValue", "callStatus")
internal open class UniffiForeignFutureStructF64Struct(
    @JvmField public var `returnValue`: Double,
    @JvmField public var `callStatus`: UniffiRustCallStatusByValue,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `returnValue` = 0.0,
        
        `callStatus` = UniffiRustCallStatusHelper.allocValue(),
        
    )

    internal class UniffiByValue(
        `returnValue`: Double,
        `callStatus`: UniffiRustCallStatusByValue,
    ): UniffiForeignFutureStructF64(`returnValue`,`callStatus`,), Structure.ByValue
}

internal typealias UniffiForeignFutureStructF64 = UniffiForeignFutureStructF64Struct

internal fun UniffiForeignFutureStructF64.uniffiSetValue(other: UniffiForeignFutureStructF64) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}
internal fun UniffiForeignFutureStructF64.uniffiSetValue(other: UniffiForeignFutureStructF64UniffiByValue) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}

internal typealias UniffiForeignFutureStructF64UniffiByValue = UniffiForeignFutureStructF64Struct.UniffiByValue
internal interface UniffiForeignFutureCompleteF64: com.sun.jna.Callback {
    public fun callback(`callbackData`: Long,`result`: UniffiForeignFutureStructF64UniffiByValue,)
}
@Structure.FieldOrder("returnValue", "callStatus")
internal open class UniffiForeignFutureStructPointerStruct(
    @JvmField public var `returnValue`: Pointer?,
    @JvmField public var `callStatus`: UniffiRustCallStatusByValue,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `returnValue` = NullPointer,
        
        `callStatus` = UniffiRustCallStatusHelper.allocValue(),
        
    )

    internal class UniffiByValue(
        `returnValue`: Pointer?,
        `callStatus`: UniffiRustCallStatusByValue,
    ): UniffiForeignFutureStructPointer(`returnValue`,`callStatus`,), Structure.ByValue
}

internal typealias UniffiForeignFutureStructPointer = UniffiForeignFutureStructPointerStruct

internal fun UniffiForeignFutureStructPointer.uniffiSetValue(other: UniffiForeignFutureStructPointer) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}
internal fun UniffiForeignFutureStructPointer.uniffiSetValue(other: UniffiForeignFutureStructPointerUniffiByValue) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}

internal typealias UniffiForeignFutureStructPointerUniffiByValue = UniffiForeignFutureStructPointerStruct.UniffiByValue
internal interface UniffiForeignFutureCompletePointer: com.sun.jna.Callback {
    public fun callback(`callbackData`: Long,`result`: UniffiForeignFutureStructPointerUniffiByValue,)
}
@Structure.FieldOrder("returnValue", "callStatus")
internal open class UniffiForeignFutureStructRustBufferStruct(
    @JvmField public var `returnValue`: RustBufferByValue,
    @JvmField public var `callStatus`: UniffiRustCallStatusByValue,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `returnValue` = RustBufferHelper.allocValue(),
        
        `callStatus` = UniffiRustCallStatusHelper.allocValue(),
        
    )

    internal class UniffiByValue(
        `returnValue`: RustBufferByValue,
        `callStatus`: UniffiRustCallStatusByValue,
    ): UniffiForeignFutureStructRustBuffer(`returnValue`,`callStatus`,), Structure.ByValue
}

internal typealias UniffiForeignFutureStructRustBuffer = UniffiForeignFutureStructRustBufferStruct

internal fun UniffiForeignFutureStructRustBuffer.uniffiSetValue(other: UniffiForeignFutureStructRustBuffer) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}
internal fun UniffiForeignFutureStructRustBuffer.uniffiSetValue(other: UniffiForeignFutureStructRustBufferUniffiByValue) {
    `returnValue` = other.`returnValue`
    `callStatus` = other.`callStatus`
}

internal typealias UniffiForeignFutureStructRustBufferUniffiByValue = UniffiForeignFutureStructRustBufferStruct.UniffiByValue
internal interface UniffiForeignFutureCompleteRustBuffer: com.sun.jna.Callback {
    public fun callback(`callbackData`: Long,`result`: UniffiForeignFutureStructRustBufferUniffiByValue,)
}
@Structure.FieldOrder("callStatus")
internal open class UniffiForeignFutureStructVoidStruct(
    @JvmField public var `callStatus`: UniffiRustCallStatusByValue,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `callStatus` = UniffiRustCallStatusHelper.allocValue(),
        
    )

    internal class UniffiByValue(
        `callStatus`: UniffiRustCallStatusByValue,
    ): UniffiForeignFutureStructVoid(`callStatus`,), Structure.ByValue
}

internal typealias UniffiForeignFutureStructVoid = UniffiForeignFutureStructVoidStruct

internal fun UniffiForeignFutureStructVoid.uniffiSetValue(other: UniffiForeignFutureStructVoid) {
    `callStatus` = other.`callStatus`
}
internal fun UniffiForeignFutureStructVoid.uniffiSetValue(other: UniffiForeignFutureStructVoidUniffiByValue) {
    `callStatus` = other.`callStatus`
}

internal typealias UniffiForeignFutureStructVoidUniffiByValue = UniffiForeignFutureStructVoidStruct.UniffiByValue
internal interface UniffiForeignFutureCompleteVoid: com.sun.jna.Callback {
    public fun callback(`callbackData`: Long,`result`: UniffiForeignFutureStructVoidUniffiByValue,)
}








































































































































































































































@Synchronized
private fun findLibraryName(componentName: String): String {
    val libOverride = System.getProperty("uniffi.component.$componentName.libraryOverride")
    if (libOverride != null) {
        return libOverride
    }
    return "bitkitcore"
}

// For large crates we prevent `MethodTooLargeException` (see #2340)
// N.B. the name of the extension is very misleading, since it is
// rather `InterfaceTooLargeException`, caused by too many methods
// in the interface for large crates.
//
// By splitting the otherwise huge interface into two parts
// * UniffiLib
// * IntegrityCheckingUniffiLib (this)
// we allow for ~2x as many methods in the UniffiLib interface.
//
// The `ffi_uniffi_contract_version` method and all checksum methods are put
// into `IntegrityCheckingUniffiLib` and these methods are called only once,
// when the library is loaded.
internal object IntegrityCheckingUniffiLib : Library {
    init {
        Native.register(IntegrityCheckingUniffiLib::class.java, findLibraryName("bitkitcore"))
        uniffiCheckContractApiVersion()
        uniffiCheckApiChecksums()
    }

    private fun uniffiCheckContractApiVersion() {
        // Get the bindings contract version from our ComponentInterface
        val bindingsContractVersion = 29
        // Get the scaffolding contract version by calling the into the dylib
        val scaffoldingContractVersion = ffi_bitkitcore_uniffi_contract_version()
        if (bindingsContractVersion != scaffoldingContractVersion) {
            throw RuntimeException("UniFFI contract version mismatch: try cleaning and rebuilding your project")
        }
    }
    private fun uniffiCheckApiChecksums() {
        if (uniffi_bitkitcore_checksum_func_activity_wipe_all() != 19332.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_add_pre_activity_metadata() != 17211.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_add_pre_activity_metadata_tags() != 28081.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_add_tags() != 63739.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_blocktank_remove_all_cjit_entries() != 40127.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_blocktank_remove_all_orders() != 38913.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_blocktank_wipe_all() != 41797.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_create_channel_request_url() != 9305.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_create_cjit_entry() != 51504.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_create_order() != 33461.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_create_withdraw_callback_url() != 39350.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_decode() != 28437.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_delete_activity_by_id() != 29867.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_delete_pre_activity_metadata() != 46621.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_derive_bitcoin_address() != 35090.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_derive_bitcoin_addresses() != 34371.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_derive_private_key() != 25155.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_entropy_to_mnemonic() != 26123.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_estimate_order_fee() != 9548.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_estimate_order_fee_full() != 13361.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_generate_mnemonic() != 19292.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_activities() != 21347.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_activities_by_tag() != 52823.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_activity_by_id() != 44227.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_all_activities_tags() != 29245.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_all_closed_channels() != 16828.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_all_pre_activity_metadata() != 25130.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_all_unique_tags() != 25431.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_bip39_suggestions() != 20658.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_bip39_wordlist() != 30814.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_cjit_entries() != 29342.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_closed_channel_by_id() != 19736.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_gift() != 386.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_info() != 43607.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_lnurl_invoice() != 5475.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_min_zero_conf_tx_fee() != 6427.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_orders() != 47460.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_payment() != 29170.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_pre_activity_metadata() != 53126.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_get_tags() != 11308.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_gift_order() != 22040.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_gift_pay() != 22142.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_init_db() != 9643.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_insert_activity() != 1510.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_is_valid_bip39_word() != 31846.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_lnurl_auth() != 58593.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_mnemonic_to_entropy() != 36669.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_mnemonic_to_seed() != 40039.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_open_channel() != 21402.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_refresh_active_cjit_entries() != 5324.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_refresh_active_orders() != 50661.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_register_device() != 14576.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_regtest_close_channel() != 48652.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_regtest_deposit() != 30356.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_regtest_get_payment() != 56623.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_regtest_mine() != 58685.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_regtest_pay() != 48342.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_remove_closed_channel_by_id() != 17150.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_remove_pre_activity_metadata_tags() != 1991.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_remove_tags() != 58873.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_reset_pre_activity_metadata_tags() != 34703.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_test_notification() != 32857.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_compose_transaction() != 25990.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_get_account_info() != 14813.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_get_address() != 42202.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_get_features() != 52582.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_handle_deep_link() != 32721.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_sign_message() != 18023.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_sign_transaction() != 59932.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_verify_message() != 44040.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_update_activity() != 42510.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_update_blocktank_url() != 52161.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_upsert_activities() != 58470.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_upsert_activity() != 32175.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_upsert_cjit_entries() != 57141.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_upsert_closed_channel() != 18711.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_upsert_closed_channels() != 2086.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_upsert_info() != 7349.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_upsert_lightning_activities() != 8564.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_upsert_onchain_activities() != 15461.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_upsert_orders() != 45856.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_upsert_pre_activity_metadata() != 12307.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_upsert_tags() != 47513.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_validate_bitcoin_address() != 56003.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_validate_mnemonic() != 31005.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_wipe_all_closed_channels() != 41511.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_wipe_all_databases() != 54605.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
    }

    // Integrity check functions only
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_activity_wipe_all(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_add_pre_activity_metadata(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_add_pre_activity_metadata_tags(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_add_tags(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_blocktank_remove_all_cjit_entries(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_blocktank_remove_all_orders(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_blocktank_wipe_all(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_create_channel_request_url(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_create_cjit_entry(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_create_order(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_create_withdraw_callback_url(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_decode(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_delete_activity_by_id(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_delete_pre_activity_metadata(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_derive_bitcoin_address(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_derive_bitcoin_addresses(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_derive_private_key(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_entropy_to_mnemonic(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_estimate_order_fee(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_estimate_order_fee_full(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_generate_mnemonic(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_activities(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_activities_by_tag(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_activity_by_id(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_all_activities_tags(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_all_closed_channels(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_all_pre_activity_metadata(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_all_unique_tags(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_bip39_suggestions(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_bip39_wordlist(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_cjit_entries(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_closed_channel_by_id(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_gift(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_info(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_lnurl_invoice(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_min_zero_conf_tx_fee(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_orders(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_payment(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_pre_activity_metadata(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_get_tags(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_gift_order(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_gift_pay(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_init_db(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_insert_activity(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_is_valid_bip39_word(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_lnurl_auth(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_mnemonic_to_entropy(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_mnemonic_to_seed(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_open_channel(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_refresh_active_cjit_entries(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_refresh_active_orders(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_register_device(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_regtest_close_channel(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_regtest_deposit(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_regtest_get_payment(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_regtest_mine(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_regtest_pay(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_remove_closed_channel_by_id(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_remove_pre_activity_metadata_tags(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_remove_tags(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_reset_pre_activity_metadata_tags(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_test_notification(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_compose_transaction(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_get_account_info(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_get_address(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_get_features(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_handle_deep_link(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_sign_message(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_sign_transaction(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_verify_message(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_update_activity(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_update_blocktank_url(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_upsert_activities(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_upsert_activity(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_upsert_cjit_entries(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_upsert_closed_channel(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_upsert_closed_channels(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_upsert_info(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_upsert_lightning_activities(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_upsert_onchain_activities(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_upsert_orders(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_upsert_pre_activity_metadata(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_upsert_tags(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_validate_bitcoin_address(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_validate_mnemonic(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_wipe_all_closed_channels(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_wipe_all_databases(
    ): Short
    @JvmStatic
    external fun ffi_bitkitcore_uniffi_contract_version(
    ): Int
}

// A JNA Library to expose the extern-C FFI definitions.
// This is an implementation detail which will be called internally by the public API.
internal object UniffiLib : Library {

    init {
        IntegrityCheckingUniffiLib
        Native.register(UniffiLib::class.java, findLibraryName("bitkitcore"))
        // No need to check the contract version and checksums, since
        // we already did that with `IntegrityCheckingUniffiLib` above.
    }
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_activity_wipe_all(
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_add_pre_activity_metadata(
        `preActivityMetadata`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_add_pre_activity_metadata_tags(
        `paymentId`: RustBufferByValue,
        `tags`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_add_tags(
        `activityId`: RustBufferByValue,
        `tags`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_blocktank_remove_all_cjit_entries(
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_blocktank_remove_all_orders(
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_blocktank_wipe_all(
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_create_channel_request_url(
        `k1`: RustBufferByValue,
        `callback`: RustBufferByValue,
        `localNodeId`: RustBufferByValue,
        `isPrivate`: Byte,
        `cancel`: Byte,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_create_cjit_entry(
        `channelSizeSat`: Long,
        `invoiceSat`: Long,
        `invoiceDescription`: RustBufferByValue,
        `nodeId`: RustBufferByValue,
        `channelExpiryWeeks`: Int,
        `options`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_create_order(
        `lspBalanceSat`: Long,
        `channelExpiryWeeks`: Int,
        `options`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_create_withdraw_callback_url(
        `k1`: RustBufferByValue,
        `callback`: RustBufferByValue,
        `paymentRequest`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_decode(
        `invoice`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_delete_activity_by_id(
        `activityId`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Byte
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_delete_pre_activity_metadata(
        `paymentId`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_derive_bitcoin_address(
        `mnemonicPhrase`: RustBufferByValue,
        `derivationPathStr`: RustBufferByValue,
        `network`: RustBufferByValue,
        `bip39Passphrase`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_derive_bitcoin_addresses(
        `mnemonicPhrase`: RustBufferByValue,
        `derivationPathStr`: RustBufferByValue,
        `network`: RustBufferByValue,
        `bip39Passphrase`: RustBufferByValue,
        `isChange`: RustBufferByValue,
        `startIndex`: RustBufferByValue,
        `count`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_derive_private_key(
        `mnemonicPhrase`: RustBufferByValue,
        `derivationPathStr`: RustBufferByValue,
        `network`: RustBufferByValue,
        `bip39Passphrase`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_entropy_to_mnemonic(
        `entropy`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_estimate_order_fee(
        `lspBalanceSat`: Long,
        `channelExpiryWeeks`: Int,
        `options`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_estimate_order_fee_full(
        `lspBalanceSat`: Long,
        `channelExpiryWeeks`: Int,
        `options`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_generate_mnemonic(
        `wordCount`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_activities(
        `filter`: RustBufferByValue,
        `txType`: RustBufferByValue,
        `tags`: RustBufferByValue,
        `search`: RustBufferByValue,
        `minDate`: RustBufferByValue,
        `maxDate`: RustBufferByValue,
        `limit`: RustBufferByValue,
        `sortDirection`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_activities_by_tag(
        `tag`: RustBufferByValue,
        `limit`: RustBufferByValue,
        `sortDirection`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_activity_by_id(
        `activityId`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_all_activities_tags(
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_all_closed_channels(
        `sortDirection`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_all_pre_activity_metadata(
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_all_unique_tags(
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_bip39_suggestions(
        `partialWord`: RustBufferByValue,
        `limit`: Int,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_bip39_wordlist(
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_cjit_entries(
        `entryIds`: RustBufferByValue,
        `filter`: RustBufferByValue,
        `refresh`: Byte,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_closed_channel_by_id(
        `channelId`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_gift(
        `giftId`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_info(
        `refresh`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_lnurl_invoice(
        `address`: RustBufferByValue,
        `amountSatoshis`: Long,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_min_zero_conf_tx_fee(
        `orderId`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_orders(
        `orderIds`: RustBufferByValue,
        `filter`: RustBufferByValue,
        `refresh`: Byte,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_payment(
        `paymentId`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_pre_activity_metadata(
        `searchKey`: RustBufferByValue,
        `searchByAddress`: Byte,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_get_tags(
        `activityId`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_gift_order(
        `clientNodeId`: RustBufferByValue,
        `code`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_gift_pay(
        `invoice`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_init_db(
        `basePath`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_insert_activity(
        `activity`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_is_valid_bip39_word(
        `word`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Byte
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_lnurl_auth(
        `domain`: RustBufferByValue,
        `k1`: RustBufferByValue,
        `callback`: RustBufferByValue,
        `bip32Mnemonic`: RustBufferByValue,
        `network`: RustBufferByValue,
        `bip39Passphrase`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_mnemonic_to_entropy(
        `mnemonicPhrase`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_mnemonic_to_seed(
        `mnemonicPhrase`: RustBufferByValue,
        `passphrase`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_open_channel(
        `orderId`: RustBufferByValue,
        `connectionString`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_refresh_active_cjit_entries(
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_refresh_active_orders(
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_register_device(
        `deviceToken`: RustBufferByValue,
        `publicKey`: RustBufferByValue,
        `features`: RustBufferByValue,
        `nodeId`: RustBufferByValue,
        `isoTimestamp`: RustBufferByValue,
        `signature`: RustBufferByValue,
        `isProduction`: RustBufferByValue,
        `customUrl`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_regtest_close_channel(
        `fundingTxId`: RustBufferByValue,
        `vout`: Int,
        `forceCloseAfterS`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_regtest_deposit(
        `address`: RustBufferByValue,
        `amountSat`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_regtest_get_payment(
        `paymentId`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_regtest_mine(
        `count`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_regtest_pay(
        `invoice`: RustBufferByValue,
        `amountSat`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_remove_closed_channel_by_id(
        `channelId`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Byte
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_remove_pre_activity_metadata_tags(
        `paymentId`: RustBufferByValue,
        `tags`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_remove_tags(
        `activityId`: RustBufferByValue,
        `tags`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_reset_pre_activity_metadata_tags(
        `paymentId`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_test_notification(
        `deviceToken`: RustBufferByValue,
        `secretMessage`: RustBufferByValue,
        `notificationType`: RustBufferByValue,
        `customUrl`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_compose_transaction(
        `outputs`: RustBufferByValue,
        `coin`: RustBufferByValue,
        `callbackUrl`: RustBufferByValue,
        `requestId`: RustBufferByValue,
        `trezorEnvironment`: RustBufferByValue,
        `push`: RustBufferByValue,
        `sequence`: RustBufferByValue,
        `account`: RustBufferByValue,
        `feeLevels`: RustBufferByValue,
        `skipPermutation`: RustBufferByValue,
        `common`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_get_account_info(
        `coin`: RustBufferByValue,
        `callbackUrl`: RustBufferByValue,
        `requestId`: RustBufferByValue,
        `trezorEnvironment`: RustBufferByValue,
        `path`: RustBufferByValue,
        `descriptor`: RustBufferByValue,
        `details`: RustBufferByValue,
        `tokens`: RustBufferByValue,
        `page`: RustBufferByValue,
        `pageSize`: RustBufferByValue,
        `from`: RustBufferByValue,
        `to`: RustBufferByValue,
        `gap`: RustBufferByValue,
        `contractFilter`: RustBufferByValue,
        `marker`: RustBufferByValue,
        `defaultAccountType`: RustBufferByValue,
        `suppressBackupWarning`: RustBufferByValue,
        `common`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_get_address(
        `path`: RustBufferByValue,
        `callbackUrl`: RustBufferByValue,
        `requestId`: RustBufferByValue,
        `trezorEnvironment`: RustBufferByValue,
        `address`: RustBufferByValue,
        `showOnTrezor`: RustBufferByValue,
        `chunkify`: RustBufferByValue,
        `useEventListener`: RustBufferByValue,
        `coin`: RustBufferByValue,
        `crossChain`: RustBufferByValue,
        `multisig`: RustBufferByValue,
        `scriptType`: RustBufferByValue,
        `unlockPath`: RustBufferByValue,
        `common`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_get_features(
        `callbackUrl`: RustBufferByValue,
        `requestId`: RustBufferByValue,
        `trezorEnvironment`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_handle_deep_link(
        `callbackUrl`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_sign_message(
        `path`: RustBufferByValue,
        `message`: RustBufferByValue,
        `callbackUrl`: RustBufferByValue,
        `requestId`: RustBufferByValue,
        `trezorEnvironment`: RustBufferByValue,
        `coin`: RustBufferByValue,
        `hex`: RustBufferByValue,
        `noScriptType`: RustBufferByValue,
        `common`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_sign_transaction(
        `coin`: RustBufferByValue,
        `inputs`: RustBufferByValue,
        `outputs`: RustBufferByValue,
        `callbackUrl`: RustBufferByValue,
        `requestId`: RustBufferByValue,
        `trezorEnvironment`: RustBufferByValue,
        `refTxs`: RustBufferByValue,
        `paymentRequests`: RustBufferByValue,
        `locktime`: RustBufferByValue,
        `version`: RustBufferByValue,
        `expiry`: RustBufferByValue,
        `versionGroupId`: RustBufferByValue,
        `overwintered`: RustBufferByValue,
        `timestamp`: RustBufferByValue,
        `branchId`: RustBufferByValue,
        `push`: RustBufferByValue,
        `amountUnit`: RustBufferByValue,
        `unlockPath`: RustBufferByValue,
        `serialize`: RustBufferByValue,
        `chunkify`: RustBufferByValue,
        `common`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_verify_message(
        `address`: RustBufferByValue,
        `signature`: RustBufferByValue,
        `message`: RustBufferByValue,
        `coin`: RustBufferByValue,
        `callbackUrl`: RustBufferByValue,
        `requestId`: RustBufferByValue,
        `trezorEnvironment`: RustBufferByValue,
        `hex`: RustBufferByValue,
        `common`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_update_activity(
        `activityId`: RustBufferByValue,
        `activity`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_update_blocktank_url(
        `newUrl`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_upsert_activities(
        `activities`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_upsert_activity(
        `activity`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_upsert_cjit_entries(
        `entries`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_upsert_closed_channel(
        `channel`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_upsert_closed_channels(
        `channels`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_upsert_info(
        `info`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_upsert_lightning_activities(
        `activities`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_upsert_onchain_activities(
        `activities`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_upsert_orders(
        `orders`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_upsert_pre_activity_metadata(
        `preActivityMetadata`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_upsert_tags(
        `activityTags`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_validate_bitcoin_address(
        `address`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_validate_mnemonic(
        `mnemonicPhrase`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_wipe_all_closed_channels(
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_wipe_all_databases(
    ): Long
    @JvmStatic
    external fun ffi_bitkitcore_rustbuffer_alloc(
        `size`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun ffi_bitkitcore_rustbuffer_from_bytes(
        `bytes`: ForeignBytesByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun ffi_bitkitcore_rustbuffer_free(
        `buf`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rustbuffer_reserve(
        `buf`: RustBufferByValue,
        `additional`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_poll_u8(
        `handle`: Long,
        `callback`: UniffiRustFutureContinuationCallback,
        `callbackData`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_cancel_u8(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_free_u8(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_complete_u8(
        `handle`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Byte
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_poll_i8(
        `handle`: Long,
        `callback`: UniffiRustFutureContinuationCallback,
        `callbackData`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_cancel_i8(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_free_i8(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_complete_i8(
        `handle`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Byte
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_poll_u16(
        `handle`: Long,
        `callback`: UniffiRustFutureContinuationCallback,
        `callbackData`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_cancel_u16(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_free_u16(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_complete_u16(
        `handle`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Short
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_poll_i16(
        `handle`: Long,
        `callback`: UniffiRustFutureContinuationCallback,
        `callbackData`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_cancel_i16(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_free_i16(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_complete_i16(
        `handle`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Short
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_poll_u32(
        `handle`: Long,
        `callback`: UniffiRustFutureContinuationCallback,
        `callbackData`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_cancel_u32(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_free_u32(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_complete_u32(
        `handle`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Int
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_poll_i32(
        `handle`: Long,
        `callback`: UniffiRustFutureContinuationCallback,
        `callbackData`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_cancel_i32(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_free_i32(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_complete_i32(
        `handle`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Int
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_poll_u64(
        `handle`: Long,
        `callback`: UniffiRustFutureContinuationCallback,
        `callbackData`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_cancel_u64(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_free_u64(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_complete_u64(
        `handle`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Long
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_poll_i64(
        `handle`: Long,
        `callback`: UniffiRustFutureContinuationCallback,
        `callbackData`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_cancel_i64(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_free_i64(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_complete_i64(
        `handle`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Long
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_poll_f32(
        `handle`: Long,
        `callback`: UniffiRustFutureContinuationCallback,
        `callbackData`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_cancel_f32(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_free_f32(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_complete_f32(
        `handle`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Float
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_poll_f64(
        `handle`: Long,
        `callback`: UniffiRustFutureContinuationCallback,
        `callbackData`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_cancel_f64(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_free_f64(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_complete_f64(
        `handle`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Double
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_poll_pointer(
        `handle`: Long,
        `callback`: UniffiRustFutureContinuationCallback,
        `callbackData`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_cancel_pointer(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_free_pointer(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_complete_pointer(
        `handle`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Pointer?
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_poll_rust_buffer(
        `handle`: Long,
        `callback`: UniffiRustFutureContinuationCallback,
        `callbackData`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_cancel_rust_buffer(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_free_rust_buffer(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_complete_rust_buffer(
        `handle`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_poll_void(
        `handle`: Long,
        `callback`: UniffiRustFutureContinuationCallback,
        `callbackData`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_cancel_void(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_free_void(
        `handle`: Long,
    ): Unit
    @JvmStatic
    external fun ffi_bitkitcore_rust_future_complete_void(
        `handle`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
}

public fun uniffiEnsureInitialized() {
    UniffiLib
}

// Public interface members begin here.



public object FfiConverterUByte: FfiConverter<UByte, Byte> {
    override fun lift(value: Byte): UByte {
        return value.toUByte()
    }

    override fun read(buf: ByteBuffer): UByte {
        return lift(buf.get())
    }

    override fun lower(value: UByte): Byte {
        return value.toByte()
    }

    override fun allocationSize(value: UByte): ULong = 1UL

    override fun write(value: UByte, buf: ByteBuffer) {
        buf.put(value.toByte())
    }
}


public object FfiConverterUInt: FfiConverter<UInt, Int> {
    override fun lift(value: Int): UInt {
        return value.toUInt()
    }

    override fun read(buf: ByteBuffer): UInt {
        return lift(buf.getInt())
    }

    override fun lower(value: UInt): Int {
        return value.toInt()
    }

    override fun allocationSize(value: UInt): ULong = 4UL

    override fun write(value: UInt, buf: ByteBuffer) {
        buf.putInt(value.toInt())
    }
}


public object FfiConverterULong: FfiConverter<ULong, Long> {
    override fun lift(value: Long): ULong {
        return value.toULong()
    }

    override fun read(buf: ByteBuffer): ULong {
        return lift(buf.getLong())
    }

    override fun lower(value: ULong): Long {
        return value.toLong()
    }

    override fun allocationSize(value: ULong): ULong = 8UL

    override fun write(value: ULong, buf: ByteBuffer) {
        buf.putLong(value.toLong())
    }
}


public object FfiConverterDouble: FfiConverter<Double, Double> {
    override fun lift(value: Double): Double {
        return value
    }

    override fun read(buf: ByteBuffer): Double {
        return buf.getDouble()
    }

    override fun lower(value: Double): Double {
        return value
    }

    override fun allocationSize(value: Double): ULong = 8UL

    override fun write(value: Double, buf: ByteBuffer) {
        buf.putDouble(value)
    }
}


public object FfiConverterBoolean: FfiConverter<Boolean, Byte> {
    override fun lift(value: Byte): Boolean {
        return value.toInt() != 0
    }

    override fun read(buf: ByteBuffer): Boolean {
        return lift(buf.get())
    }

    override fun lower(value: Boolean): Byte {
        return if (value) 1.toByte() else 0.toByte()
    }

    override fun allocationSize(value: Boolean): ULong = 1UL

    override fun write(value: Boolean, buf: ByteBuffer) {
        buf.put(lower(value))
    }
}


public object FfiConverterString: FfiConverter<String, RustBufferByValue> {
    // Note: we don't inherit from FfiConverterRustBuffer, because we use a
    // special encoding when lowering/lifting.  We can use `RustBuffer.len` to
    // store our length and avoid writing it out to the buffer.
    override fun lift(value: RustBufferByValue): String {
        try {
            require(value.len <= Int.MAX_VALUE) {
        val length = value.len
        "cannot handle RustBuffer longer than Int.MAX_VALUE bytes: length is $length"
    }
            val byteArr =  value.asByteBuffer()!!.get(value.len.toInt())
            return byteArr.decodeToString()
        } finally {
            RustBufferHelper.free(value)
        }
    }

    override fun read(buf: ByteBuffer): String {
        val len = buf.getInt()
        val byteArr = buf.get(len)
        return byteArr.decodeToString()
    }

    override fun lower(value: String): RustBufferByValue {
        // TODO: prevent allocating a new byte array here
        val encoded = value.encodeToByteArray(throwOnInvalidSequence = true)
        return RustBufferHelper.allocValue(encoded.size.toULong()).apply {
            asByteBuffer()!!.put(encoded)
        }
    }

    // We aren't sure exactly how many bytes our string will be once it's UTF-8
    // encoded.  Allocate 3 bytes per UTF-16 code unit which will always be
    // enough.
    override fun allocationSize(value: String): ULong {
        val sizeForLength = 4UL
        val sizeForString = value.length.toULong() * 3UL
        return sizeForLength + sizeForString
    }

    override fun write(value: String, buf: ByteBuffer) {
        // TODO: prevent allocating a new byte array here
        val encoded = value.encodeToByteArray(throwOnInvalidSequence = true)
        buf.putInt(encoded.size)
        buf.put(encoded)
    }
}


public object FfiConverterByteArray: FfiConverterRustBuffer<ByteArray> {
    override fun read(buf: ByteBuffer): ByteArray {
        val len = buf.getInt()
        val byteArr = buf.get(len)
        return byteArr
    }
    override fun allocationSize(value: ByteArray): ULong {
        return 4UL + value.size.toULong()
    }
    override fun write(value: ByteArray, buf: ByteBuffer) {
        buf.putInt(value.size)
        buf.put(value)
    }
}




public object FfiConverterTypeAccountAddresses: FfiConverterRustBuffer<AccountAddresses> {
    override fun read(buf: ByteBuffer): AccountAddresses {
        return AccountAddresses(
            FfiConverterSequenceTypeAddressInfo.read(buf),
            FfiConverterSequenceTypeAddressInfo.read(buf),
            FfiConverterSequenceTypeAddressInfo.read(buf),
        )
    }

    override fun allocationSize(value: AccountAddresses): ULong = (
            FfiConverterSequenceTypeAddressInfo.allocationSize(value.`used`) +
            FfiConverterSequenceTypeAddressInfo.allocationSize(value.`unused`) +
            FfiConverterSequenceTypeAddressInfo.allocationSize(value.`change`)
    )

    override fun write(value: AccountAddresses, buf: ByteBuffer) {
        FfiConverterSequenceTypeAddressInfo.write(value.`used`, buf)
        FfiConverterSequenceTypeAddressInfo.write(value.`unused`, buf)
        FfiConverterSequenceTypeAddressInfo.write(value.`change`, buf)
    }
}




public object FfiConverterTypeAccountInfoResponse: FfiConverterRustBuffer<AccountInfoResponse> {
    override fun read(buf: ByteBuffer): AccountInfoResponse {
        return AccountInfoResponse(
            FfiConverterUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: AccountInfoResponse): ULong = (
            FfiConverterUInt.allocationSize(value.`id`) +
            FfiConverterString.allocationSize(value.`path`) +
            FfiConverterString.allocationSize(value.`descriptor`) +
            FfiConverterOptionalString.allocationSize(value.`legacyXpub`) +
            FfiConverterString.allocationSize(value.`balance`) +
            FfiConverterString.allocationSize(value.`availableBalance`)
    )

    override fun write(value: AccountInfoResponse, buf: ByteBuffer) {
        FfiConverterUInt.write(value.`id`, buf)
        FfiConverterString.write(value.`path`, buf)
        FfiConverterString.write(value.`descriptor`, buf)
        FfiConverterOptionalString.write(value.`legacyXpub`, buf)
        FfiConverterString.write(value.`balance`, buf)
        FfiConverterString.write(value.`availableBalance`, buf)
    }
}




public object FfiConverterTypeAccountUtxo: FfiConverterRustBuffer<AccountUtxo> {
    override fun read(buf: ByteBuffer): AccountUtxo {
        return AccountUtxo(
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalUInt.read(buf),
        )
    }

    override fun allocationSize(value: AccountUtxo): ULong = (
            FfiConverterString.allocationSize(value.`txid`) +
            FfiConverterUInt.allocationSize(value.`vout`) +
            FfiConverterString.allocationSize(value.`amount`) +
            FfiConverterOptionalUInt.allocationSize(value.`blockHeight`) +
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterString.allocationSize(value.`path`) +
            FfiConverterOptionalUInt.allocationSize(value.`confirmations`)
    )

    override fun write(value: AccountUtxo, buf: ByteBuffer) {
        FfiConverterString.write(value.`txid`, buf)
        FfiConverterUInt.write(value.`vout`, buf)
        FfiConverterString.write(value.`amount`, buf)
        FfiConverterOptionalUInt.write(value.`blockHeight`, buf)
        FfiConverterString.write(value.`address`, buf)
        FfiConverterString.write(value.`path`, buf)
        FfiConverterOptionalUInt.write(value.`confirmations`, buf)
    }
}




public object FfiConverterTypeActivityTags: FfiConverterRustBuffer<ActivityTags> {
    override fun read(buf: ByteBuffer): ActivityTags {
        return ActivityTags(
            FfiConverterString.read(buf),
            FfiConverterSequenceString.read(buf),
        )
    }

    override fun allocationSize(value: ActivityTags): ULong = (
            FfiConverterString.allocationSize(value.`activityId`) +
            FfiConverterSequenceString.allocationSize(value.`tags`)
    )

    override fun write(value: ActivityTags, buf: ByteBuffer) {
        FfiConverterString.write(value.`activityId`, buf)
        FfiConverterSequenceString.write(value.`tags`, buf)
    }
}




public object FfiConverterTypeAddressInfo: FfiConverterRustBuffer<AddressInfo> {
    override fun read(buf: ByteBuffer): AddressInfo {
        return AddressInfo(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
        )
    }

    override fun allocationSize(value: AddressInfo): ULong = (
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterString.allocationSize(value.`path`) +
            FfiConverterUInt.allocationSize(value.`transfers`)
    )

    override fun write(value: AddressInfo, buf: ByteBuffer) {
        FfiConverterString.write(value.`address`, buf)
        FfiConverterString.write(value.`path`, buf)
        FfiConverterUInt.write(value.`transfers`, buf)
    }
}




public object FfiConverterTypeAddressResponse: FfiConverterRustBuffer<AddressResponse> {
    override fun read(buf: ByteBuffer): AddressResponse {
        return AddressResponse(
            FfiConverterString.read(buf),
            FfiConverterSequenceUInt.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: AddressResponse): ULong = (
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterSequenceUInt.allocationSize(value.`path`) +
            FfiConverterString.allocationSize(value.`serializedPath`)
    )

    override fun write(value: AddressResponse, buf: ByteBuffer) {
        FfiConverterString.write(value.`address`, buf)
        FfiConverterSequenceUInt.write(value.`path`, buf)
        FfiConverterString.write(value.`serializedPath`, buf)
    }
}




public object FfiConverterTypeClosedChannelDetails: FfiConverterRustBuffer<ClosedChannelDetails> {
    override fun read(buf: ByteBuffer): ClosedChannelDetails {
        return ClosedChannelDetails(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: ClosedChannelDetails): ULong = (
            FfiConverterString.allocationSize(value.`channelId`) +
            FfiConverterString.allocationSize(value.`counterpartyNodeId`) +
            FfiConverterString.allocationSize(value.`fundingTxoTxid`) +
            FfiConverterUInt.allocationSize(value.`fundingTxoIndex`) +
            FfiConverterULong.allocationSize(value.`channelValueSats`) +
            FfiConverterULong.allocationSize(value.`closedAt`) +
            FfiConverterULong.allocationSize(value.`outboundCapacityMsat`) +
            FfiConverterULong.allocationSize(value.`inboundCapacityMsat`) +
            FfiConverterULong.allocationSize(value.`counterpartyUnspendablePunishmentReserve`) +
            FfiConverterULong.allocationSize(value.`unspendablePunishmentReserve`) +
            FfiConverterUInt.allocationSize(value.`forwardingFeeProportionalMillionths`) +
            FfiConverterUInt.allocationSize(value.`forwardingFeeBaseMsat`) +
            FfiConverterString.allocationSize(value.`channelName`) +
            FfiConverterString.allocationSize(value.`channelClosureReason`)
    )

    override fun write(value: ClosedChannelDetails, buf: ByteBuffer) {
        FfiConverterString.write(value.`channelId`, buf)
        FfiConverterString.write(value.`counterpartyNodeId`, buf)
        FfiConverterString.write(value.`fundingTxoTxid`, buf)
        FfiConverterUInt.write(value.`fundingTxoIndex`, buf)
        FfiConverterULong.write(value.`channelValueSats`, buf)
        FfiConverterULong.write(value.`closedAt`, buf)
        FfiConverterULong.write(value.`outboundCapacityMsat`, buf)
        FfiConverterULong.write(value.`inboundCapacityMsat`, buf)
        FfiConverterULong.write(value.`counterpartyUnspendablePunishmentReserve`, buf)
        FfiConverterULong.write(value.`unspendablePunishmentReserve`, buf)
        FfiConverterUInt.write(value.`forwardingFeeProportionalMillionths`, buf)
        FfiConverterUInt.write(value.`forwardingFeeBaseMsat`, buf)
        FfiConverterString.write(value.`channelName`, buf)
        FfiConverterString.write(value.`channelClosureReason`, buf)
    }
}




public object FfiConverterTypeCoinPurchaseMemo: FfiConverterRustBuffer<CoinPurchaseMemo> {
    override fun read(buf: ByteBuffer): CoinPurchaseMemo {
        return CoinPurchaseMemo(
            FfiConverterUInt.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: CoinPurchaseMemo): ULong = (
            FfiConverterUInt.allocationSize(value.`coinType`) +
            FfiConverterULong.allocationSize(value.`amount`) +
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterString.allocationSize(value.`mac`)
    )

    override fun write(value: CoinPurchaseMemo, buf: ByteBuffer) {
        FfiConverterUInt.write(value.`coinType`, buf)
        FfiConverterULong.write(value.`amount`, buf)
        FfiConverterString.write(value.`address`, buf)
        FfiConverterString.write(value.`mac`, buf)
    }
}




public object FfiConverterTypeCommonParams: FfiConverterRustBuffer<CommonParams> {
    override fun read(buf: ByteBuffer): CommonParams {
        return CommonParams(
            FfiConverterOptionalTypeDeviceParams.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalBoolean.read(buf),
        )
    }

    override fun allocationSize(value: CommonParams): ULong = (
            FfiConverterOptionalTypeDeviceParams.allocationSize(value.`device`) +
            FfiConverterOptionalBoolean.allocationSize(value.`useEmptyPassphrase`) +
            FfiConverterOptionalBoolean.allocationSize(value.`allowSeedlessDevice`) +
            FfiConverterOptionalBoolean.allocationSize(value.`skipFinalReload`)
    )

    override fun write(value: CommonParams, buf: ByteBuffer) {
        FfiConverterOptionalTypeDeviceParams.write(value.`device`, buf)
        FfiConverterOptionalBoolean.write(value.`useEmptyPassphrase`, buf)
        FfiConverterOptionalBoolean.write(value.`allowSeedlessDevice`, buf)
        FfiConverterOptionalBoolean.write(value.`skipFinalReload`, buf)
    }
}




public object FfiConverterTypeComposeAccount: FfiConverterRustBuffer<ComposeAccount> {
    override fun read(buf: ByteBuffer): ComposeAccount {
        return ComposeAccount(
            FfiConverterString.read(buf),
            FfiConverterTypeAccountAddresses.read(buf),
            FfiConverterSequenceTypeAccountUtxo.read(buf),
        )
    }

    override fun allocationSize(value: ComposeAccount): ULong = (
            FfiConverterString.allocationSize(value.`path`) +
            FfiConverterTypeAccountAddresses.allocationSize(value.`addresses`) +
            FfiConverterSequenceTypeAccountUtxo.allocationSize(value.`utxo`)
    )

    override fun write(value: ComposeAccount, buf: ByteBuffer) {
        FfiConverterString.write(value.`path`, buf)
        FfiConverterTypeAccountAddresses.write(value.`addresses`, buf)
        FfiConverterSequenceTypeAccountUtxo.write(value.`utxo`, buf)
    }
}




public object FfiConverterTypeCreateCjitOptions: FfiConverterRustBuffer<CreateCjitOptions> {
    override fun read(buf: ByteBuffer): CreateCjitOptions {
        return CreateCjitOptions(
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
        )
    }

    override fun allocationSize(value: CreateCjitOptions): ULong = (
            FfiConverterOptionalString.allocationSize(value.`source`) +
            FfiConverterOptionalString.allocationSize(value.`discountCode`)
    )

    override fun write(value: CreateCjitOptions, buf: ByteBuffer) {
        FfiConverterOptionalString.write(value.`source`, buf)
        FfiConverterOptionalString.write(value.`discountCode`, buf)
    }
}




public object FfiConverterTypeCreateOrderOptions: FfiConverterRustBuffer<CreateOrderOptions> {
    override fun read(buf: ByteBuffer): CreateOrderOptions {
        return CreateOrderOptions(
            FfiConverterULong.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterBoolean.read(buf),
        )
    }

    override fun allocationSize(value: CreateOrderOptions): ULong = (
            FfiConverterULong.allocationSize(value.`clientBalanceSat`) +
            FfiConverterOptionalString.allocationSize(value.`lspNodeId`) +
            FfiConverterString.allocationSize(value.`couponCode`) +
            FfiConverterOptionalString.allocationSize(value.`source`) +
            FfiConverterOptionalString.allocationSize(value.`discountCode`) +
            FfiConverterBoolean.allocationSize(value.`zeroConf`) +
            FfiConverterOptionalBoolean.allocationSize(value.`zeroConfPayment`) +
            FfiConverterBoolean.allocationSize(value.`zeroReserve`) +
            FfiConverterOptionalString.allocationSize(value.`clientNodeId`) +
            FfiConverterOptionalString.allocationSize(value.`signature`) +
            FfiConverterOptionalString.allocationSize(value.`timestamp`) +
            FfiConverterOptionalString.allocationSize(value.`refundOnchainAddress`) +
            FfiConverterBoolean.allocationSize(value.`announceChannel`)
    )

    override fun write(value: CreateOrderOptions, buf: ByteBuffer) {
        FfiConverterULong.write(value.`clientBalanceSat`, buf)
        FfiConverterOptionalString.write(value.`lspNodeId`, buf)
        FfiConverterString.write(value.`couponCode`, buf)
        FfiConverterOptionalString.write(value.`source`, buf)
        FfiConverterOptionalString.write(value.`discountCode`, buf)
        FfiConverterBoolean.write(value.`zeroConf`, buf)
        FfiConverterOptionalBoolean.write(value.`zeroConfPayment`, buf)
        FfiConverterBoolean.write(value.`zeroReserve`, buf)
        FfiConverterOptionalString.write(value.`clientNodeId`, buf)
        FfiConverterOptionalString.write(value.`signature`, buf)
        FfiConverterOptionalString.write(value.`timestamp`, buf)
        FfiConverterOptionalString.write(value.`refundOnchainAddress`, buf)
        FfiConverterBoolean.write(value.`announceChannel`, buf)
    }
}




public object FfiConverterTypeDeepLinkResult: FfiConverterRustBuffer<DeepLinkResult> {
    override fun read(buf: ByteBuffer): DeepLinkResult {
        return DeepLinkResult(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: DeepLinkResult): ULong = (
            FfiConverterString.allocationSize(value.`url`) +
            FfiConverterString.allocationSize(value.`requestId`)
    )

    override fun write(value: DeepLinkResult, buf: ByteBuffer) {
        FfiConverterString.write(value.`url`, buf)
        FfiConverterString.write(value.`requestId`, buf)
    }
}




public object FfiConverterTypeDeviceParams: FfiConverterRustBuffer<DeviceParams> {
    override fun read(buf: ByteBuffer): DeviceParams {
        return DeviceParams(
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalUInt.read(buf),
        )
    }

    override fun allocationSize(value: DeviceParams): ULong = (
            FfiConverterOptionalString.allocationSize(value.`path`) +
            FfiConverterOptionalUInt.allocationSize(value.`instance`)
    )

    override fun write(value: DeviceParams, buf: ByteBuffer) {
        FfiConverterOptionalString.write(value.`path`, buf)
        FfiConverterOptionalUInt.write(value.`instance`, buf)
    }
}




public object FfiConverterTypeErrorData: FfiConverterRustBuffer<ErrorData> {
    override fun read(buf: ByteBuffer): ErrorData {
        return ErrorData(
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: ErrorData): ULong = (
            FfiConverterString.allocationSize(value.`errorDetails`)
    )

    override fun write(value: ErrorData, buf: ByteBuffer) {
        FfiConverterString.write(value.`errorDetails`, buf)
    }
}




public object FfiConverterTypeFeatureResponse: FfiConverterRustBuffer<FeatureResponse> {
    override fun read(buf: ByteBuffer): FeatureResponse {
        return FeatureResponse(
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalSequenceString.read(buf),
        )
    }

    override fun allocationSize(value: FeatureResponse): ULong = (
            FfiConverterString.allocationSize(value.`vendor`) +
            FfiConverterUInt.allocationSize(value.`majorVersion`) +
            FfiConverterUInt.allocationSize(value.`minorVersion`) +
            FfiConverterUInt.allocationSize(value.`patchVersion`) +
            FfiConverterString.allocationSize(value.`deviceId`) +
            FfiConverterOptionalSequenceString.allocationSize(value.`capabilities`)
    )

    override fun write(value: FeatureResponse, buf: ByteBuffer) {
        FfiConverterString.write(value.`vendor`, buf)
        FfiConverterUInt.write(value.`majorVersion`, buf)
        FfiConverterUInt.write(value.`minorVersion`, buf)
        FfiConverterUInt.write(value.`patchVersion`, buf)
        FfiConverterString.write(value.`deviceId`, buf)
        FfiConverterOptionalSequenceString.write(value.`capabilities`, buf)
    }
}




public object FfiConverterTypeFeeLevel: FfiConverterRustBuffer<FeeLevel> {
    override fun read(buf: ByteBuffer): FeeLevel {
        return FeeLevel(
            FfiConverterString.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalBoolean.read(buf),
        )
    }

    override fun allocationSize(value: FeeLevel): ULong = (
            FfiConverterString.allocationSize(value.`feePerUnit`) +
            FfiConverterOptionalUInt.allocationSize(value.`baseFee`) +
            FfiConverterOptionalBoolean.allocationSize(value.`floorBaseFee`)
    )

    override fun write(value: FeeLevel, buf: ByteBuffer) {
        FfiConverterString.write(value.`feePerUnit`, buf)
        FfiConverterOptionalUInt.write(value.`baseFee`, buf)
        FfiConverterOptionalBoolean.write(value.`floorBaseFee`, buf)
    }
}




public object FfiConverterTypeFeeRates: FfiConverterRustBuffer<FeeRates> {
    override fun read(buf: ByteBuffer): FeeRates {
        return FeeRates(
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
        )
    }

    override fun allocationSize(value: FeeRates): ULong = (
            FfiConverterUInt.allocationSize(value.`fast`) +
            FfiConverterUInt.allocationSize(value.`mid`) +
            FfiConverterUInt.allocationSize(value.`slow`)
    )

    override fun write(value: FeeRates, buf: ByteBuffer) {
        FfiConverterUInt.write(value.`fast`, buf)
        FfiConverterUInt.write(value.`mid`, buf)
        FfiConverterUInt.write(value.`slow`, buf)
    }
}




public object FfiConverterTypeFundingTx: FfiConverterRustBuffer<FundingTx> {
    override fun read(buf: ByteBuffer): FundingTx {
        return FundingTx(
            FfiConverterString.read(buf),
            FfiConverterULong.read(buf),
        )
    }

    override fun allocationSize(value: FundingTx): ULong = (
            FfiConverterString.allocationSize(value.`id`) +
            FfiConverterULong.allocationSize(value.`vout`)
    )

    override fun write(value: FundingTx, buf: ByteBuffer) {
        FfiConverterString.write(value.`id`, buf)
        FfiConverterULong.write(value.`vout`, buf)
    }
}




public object FfiConverterTypeGetAddressResponse: FfiConverterRustBuffer<GetAddressResponse> {
    override fun read(buf: ByteBuffer): GetAddressResponse {
        return GetAddressResponse(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: GetAddressResponse): ULong = (
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterString.allocationSize(value.`path`) +
            FfiConverterString.allocationSize(value.`publicKey`)
    )

    override fun write(value: GetAddressResponse, buf: ByteBuffer) {
        FfiConverterString.write(value.`address`, buf)
        FfiConverterString.write(value.`path`, buf)
        FfiConverterString.write(value.`publicKey`, buf)
    }
}




public object FfiConverterTypeGetAddressesResponse: FfiConverterRustBuffer<GetAddressesResponse> {
    override fun read(buf: ByteBuffer): GetAddressesResponse {
        return GetAddressesResponse(
            FfiConverterSequenceTypeGetAddressResponse.read(buf),
        )
    }

    override fun allocationSize(value: GetAddressesResponse): ULong = (
            FfiConverterSequenceTypeGetAddressResponse.allocationSize(value.`addresses`)
    )

    override fun write(value: GetAddressesResponse, buf: ByteBuffer) {
        FfiConverterSequenceTypeGetAddressResponse.write(value.`addresses`, buf)
    }
}




public object FfiConverterTypeHDNodePathType: FfiConverterRustBuffer<HdNodePathType> {
    override fun read(buf: ByteBuffer): HdNodePathType {
        return HdNodePathType(
            FfiConverterTypeHDNodeTypeOrString.read(buf),
            FfiConverterSequenceUInt.read(buf),
        )
    }

    override fun allocationSize(value: HdNodePathType): ULong = (
            FfiConverterTypeHDNodeTypeOrString.allocationSize(value.`node`) +
            FfiConverterSequenceUInt.allocationSize(value.`addressN`)
    )

    override fun write(value: HdNodePathType, buf: ByteBuffer) {
        FfiConverterTypeHDNodeTypeOrString.write(value.`node`, buf)
        FfiConverterSequenceUInt.write(value.`addressN`, buf)
    }
}




public object FfiConverterTypeHDNodeType: FfiConverterRustBuffer<HdNodeType> {
    override fun read(buf: ByteBuffer): HdNodeType {
        return HdNodeType(
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalSequenceUInt.read(buf),
        )
    }

    override fun allocationSize(value: HdNodeType): ULong = (
            FfiConverterUInt.allocationSize(value.`depth`) +
            FfiConverterUInt.allocationSize(value.`fingerprint`) +
            FfiConverterUInt.allocationSize(value.`childNum`) +
            FfiConverterString.allocationSize(value.`chainCode`) +
            FfiConverterString.allocationSize(value.`publicKey`) +
            FfiConverterOptionalString.allocationSize(value.`privateKey`) +
            FfiConverterOptionalSequenceUInt.allocationSize(value.`addressN`)
    )

    override fun write(value: HdNodeType, buf: ByteBuffer) {
        FfiConverterUInt.write(value.`depth`, buf)
        FfiConverterUInt.write(value.`fingerprint`, buf)
        FfiConverterUInt.write(value.`childNum`, buf)
        FfiConverterString.write(value.`chainCode`, buf)
        FfiConverterString.write(value.`publicKey`, buf)
        FfiConverterOptionalString.write(value.`privateKey`, buf)
        FfiConverterOptionalSequenceUInt.write(value.`addressN`, buf)
    }
}




public object FfiConverterTypeIBt0ConfMinTxFeeWindow: FfiConverterRustBuffer<IBt0ConfMinTxFeeWindow> {
    override fun read(buf: ByteBuffer): IBt0ConfMinTxFeeWindow {
        return IBt0ConfMinTxFeeWindow(
            FfiConverterDouble.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: IBt0ConfMinTxFeeWindow): ULong = (
            FfiConverterDouble.allocationSize(value.`satPerVbyte`) +
            FfiConverterString.allocationSize(value.`validityEndsAt`)
    )

    override fun write(value: IBt0ConfMinTxFeeWindow, buf: ByteBuffer) {
        FfiConverterDouble.write(value.`satPerVbyte`, buf)
        FfiConverterString.write(value.`validityEndsAt`, buf)
    }
}




public object FfiConverterTypeIBtBolt11Invoice: FfiConverterRustBuffer<IBtBolt11Invoice> {
    override fun read(buf: ByteBuffer): IBtBolt11Invoice {
        return IBtBolt11Invoice(
            FfiConverterString.read(buf),
            FfiConverterTypeBtBolt11InvoiceState.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: IBtBolt11Invoice): ULong = (
            FfiConverterString.allocationSize(value.`request`) +
            FfiConverterTypeBtBolt11InvoiceState.allocationSize(value.`state`) +
            FfiConverterString.allocationSize(value.`expiresAt`) +
            FfiConverterString.allocationSize(value.`updatedAt`)
    )

    override fun write(value: IBtBolt11Invoice, buf: ByteBuffer) {
        FfiConverterString.write(value.`request`, buf)
        FfiConverterTypeBtBolt11InvoiceState.write(value.`state`, buf)
        FfiConverterString.write(value.`expiresAt`, buf)
        FfiConverterString.write(value.`updatedAt`, buf)
    }
}




public object FfiConverterTypeIBtChannel: FfiConverterRustBuffer<IBtChannel> {
    override fun read(buf: ByteBuffer): IBtChannel {
        return IBtChannel(
            FfiConverterTypeBtOpenChannelState.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterTypeFundingTx.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalTypeIBtChannelClose.read(buf),
            FfiConverterOptionalString.read(buf),
        )
    }

    override fun allocationSize(value: IBtChannel): ULong = (
            FfiConverterTypeBtOpenChannelState.allocationSize(value.`state`) +
            FfiConverterString.allocationSize(value.`lspNodePubkey`) +
            FfiConverterString.allocationSize(value.`clientNodePubkey`) +
            FfiConverterBoolean.allocationSize(value.`announceChannel`) +
            FfiConverterTypeFundingTx.allocationSize(value.`fundingTx`) +
            FfiConverterOptionalString.allocationSize(value.`closingTxId`) +
            FfiConverterOptionalTypeIBtChannelClose.allocationSize(value.`close`) +
            FfiConverterOptionalString.allocationSize(value.`shortChannelId`)
    )

    override fun write(value: IBtChannel, buf: ByteBuffer) {
        FfiConverterTypeBtOpenChannelState.write(value.`state`, buf)
        FfiConverterString.write(value.`lspNodePubkey`, buf)
        FfiConverterString.write(value.`clientNodePubkey`, buf)
        FfiConverterBoolean.write(value.`announceChannel`, buf)
        FfiConverterTypeFundingTx.write(value.`fundingTx`, buf)
        FfiConverterOptionalString.write(value.`closingTxId`, buf)
        FfiConverterOptionalTypeIBtChannelClose.write(value.`close`, buf)
        FfiConverterOptionalString.write(value.`shortChannelId`, buf)
    }
}




public object FfiConverterTypeIBtChannelClose: FfiConverterRustBuffer<IBtChannelClose> {
    override fun read(buf: ByteBuffer): IBtChannelClose {
        return IBtChannelClose(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: IBtChannelClose): ULong = (
            FfiConverterString.allocationSize(value.`txId`) +
            FfiConverterString.allocationSize(value.`closeType`) +
            FfiConverterString.allocationSize(value.`initiator`) +
            FfiConverterString.allocationSize(value.`registeredAt`)
    )

    override fun write(value: IBtChannelClose, buf: ByteBuffer) {
        FfiConverterString.write(value.`txId`, buf)
        FfiConverterString.write(value.`closeType`, buf)
        FfiConverterString.write(value.`initiator`, buf)
        FfiConverterString.write(value.`registeredAt`, buf)
    }
}




public object FfiConverterTypeIBtEstimateFeeResponse: FfiConverterRustBuffer<IBtEstimateFeeResponse> {
    override fun read(buf: ByteBuffer): IBtEstimateFeeResponse {
        return IBtEstimateFeeResponse(
            FfiConverterULong.read(buf),
            FfiConverterTypeIBt0ConfMinTxFeeWindow.read(buf),
        )
    }

    override fun allocationSize(value: IBtEstimateFeeResponse): ULong = (
            FfiConverterULong.allocationSize(value.`feeSat`) +
            FfiConverterTypeIBt0ConfMinTxFeeWindow.allocationSize(value.`min0ConfTxFee`)
    )

    override fun write(value: IBtEstimateFeeResponse, buf: ByteBuffer) {
        FfiConverterULong.write(value.`feeSat`, buf)
        FfiConverterTypeIBt0ConfMinTxFeeWindow.write(value.`min0ConfTxFee`, buf)
    }
}




public object FfiConverterTypeIBtEstimateFeeResponse2: FfiConverterRustBuffer<IBtEstimateFeeResponse2> {
    override fun read(buf: ByteBuffer): IBtEstimateFeeResponse2 {
        return IBtEstimateFeeResponse2(
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterTypeIBt0ConfMinTxFeeWindow.read(buf),
        )
    }

    override fun allocationSize(value: IBtEstimateFeeResponse2): ULong = (
            FfiConverterULong.allocationSize(value.`feeSat`) +
            FfiConverterULong.allocationSize(value.`networkFeeSat`) +
            FfiConverterULong.allocationSize(value.`serviceFeeSat`) +
            FfiConverterTypeIBt0ConfMinTxFeeWindow.allocationSize(value.`min0ConfTxFee`)
    )

    override fun write(value: IBtEstimateFeeResponse2, buf: ByteBuffer) {
        FfiConverterULong.write(value.`feeSat`, buf)
        FfiConverterULong.write(value.`networkFeeSat`, buf)
        FfiConverterULong.write(value.`serviceFeeSat`, buf)
        FfiConverterTypeIBt0ConfMinTxFeeWindow.write(value.`min0ConfTxFee`, buf)
    }
}




public object FfiConverterTypeIBtInfo: FfiConverterRustBuffer<IBtInfo> {
    override fun read(buf: ByteBuffer): IBtInfo {
        return IBtInfo(
            FfiConverterUInt.read(buf),
            FfiConverterSequenceTypeILspNode.read(buf),
            FfiConverterTypeIBtInfoOptions.read(buf),
            FfiConverterTypeIBtInfoVersions.read(buf),
            FfiConverterTypeIBtInfoOnchain.read(buf),
        )
    }

    override fun allocationSize(value: IBtInfo): ULong = (
            FfiConverterUInt.allocationSize(value.`version`) +
            FfiConverterSequenceTypeILspNode.allocationSize(value.`nodes`) +
            FfiConverterTypeIBtInfoOptions.allocationSize(value.`options`) +
            FfiConverterTypeIBtInfoVersions.allocationSize(value.`versions`) +
            FfiConverterTypeIBtInfoOnchain.allocationSize(value.`onchain`)
    )

    override fun write(value: IBtInfo, buf: ByteBuffer) {
        FfiConverterUInt.write(value.`version`, buf)
        FfiConverterSequenceTypeILspNode.write(value.`nodes`, buf)
        FfiConverterTypeIBtInfoOptions.write(value.`options`, buf)
        FfiConverterTypeIBtInfoVersions.write(value.`versions`, buf)
        FfiConverterTypeIBtInfoOnchain.write(value.`onchain`, buf)
    }
}




public object FfiConverterTypeIBtInfoOnchain: FfiConverterRustBuffer<IBtInfoOnchain> {
    override fun read(buf: ByteBuffer): IBtInfoOnchain {
        return IBtInfoOnchain(
            FfiConverterTypeBitcoinNetworkEnum.read(buf),
            FfiConverterTypeFeeRates.read(buf),
        )
    }

    override fun allocationSize(value: IBtInfoOnchain): ULong = (
            FfiConverterTypeBitcoinNetworkEnum.allocationSize(value.`network`) +
            FfiConverterTypeFeeRates.allocationSize(value.`feeRates`)
    )

    override fun write(value: IBtInfoOnchain, buf: ByteBuffer) {
        FfiConverterTypeBitcoinNetworkEnum.write(value.`network`, buf)
        FfiConverterTypeFeeRates.write(value.`feeRates`, buf)
    }
}




public object FfiConverterTypeIBtInfoOptions: FfiConverterRustBuffer<IBtInfoOptions> {
    override fun read(buf: ByteBuffer): IBtInfoOptions {
        return IBtInfoOptions(
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
        )
    }

    override fun allocationSize(value: IBtInfoOptions): ULong = (
            FfiConverterULong.allocationSize(value.`minChannelSizeSat`) +
            FfiConverterULong.allocationSize(value.`maxChannelSizeSat`) +
            FfiConverterUInt.allocationSize(value.`minExpiryWeeks`) +
            FfiConverterUInt.allocationSize(value.`maxExpiryWeeks`) +
            FfiConverterUInt.allocationSize(value.`minPaymentConfirmations`) +
            FfiConverterUInt.allocationSize(value.`minHighRiskPaymentConfirmations`) +
            FfiConverterULong.allocationSize(value.`max0ConfClientBalanceSat`) +
            FfiConverterULong.allocationSize(value.`maxClientBalanceSat`)
    )

    override fun write(value: IBtInfoOptions, buf: ByteBuffer) {
        FfiConverterULong.write(value.`minChannelSizeSat`, buf)
        FfiConverterULong.write(value.`maxChannelSizeSat`, buf)
        FfiConverterUInt.write(value.`minExpiryWeeks`, buf)
        FfiConverterUInt.write(value.`maxExpiryWeeks`, buf)
        FfiConverterUInt.write(value.`minPaymentConfirmations`, buf)
        FfiConverterUInt.write(value.`minHighRiskPaymentConfirmations`, buf)
        FfiConverterULong.write(value.`max0ConfClientBalanceSat`, buf)
        FfiConverterULong.write(value.`maxClientBalanceSat`, buf)
    }
}




public object FfiConverterTypeIBtInfoVersions: FfiConverterRustBuffer<IBtInfoVersions> {
    override fun read(buf: ByteBuffer): IBtInfoVersions {
        return IBtInfoVersions(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: IBtInfoVersions): ULong = (
            FfiConverterString.allocationSize(value.`http`) +
            FfiConverterString.allocationSize(value.`btc`) +
            FfiConverterString.allocationSize(value.`ln2`)
    )

    override fun write(value: IBtInfoVersions, buf: ByteBuffer) {
        FfiConverterString.write(value.`http`, buf)
        FfiConverterString.write(value.`btc`, buf)
        FfiConverterString.write(value.`ln2`, buf)
    }
}




public object FfiConverterTypeIBtOnchainTransaction: FfiConverterRustBuffer<IBtOnchainTransaction> {
    override fun read(buf: ByteBuffer): IBtOnchainTransaction {
        return IBtOnchainTransaction(
            FfiConverterULong.read(buf),
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterDouble.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: IBtOnchainTransaction): ULong = (
            FfiConverterULong.allocationSize(value.`amountSat`) +
            FfiConverterString.allocationSize(value.`txId`) +
            FfiConverterUInt.allocationSize(value.`vout`) +
            FfiConverterOptionalUInt.allocationSize(value.`blockHeight`) +
            FfiConverterUInt.allocationSize(value.`blockConfirmationCount`) +
            FfiConverterDouble.allocationSize(value.`feeRateSatPerVbyte`) +
            FfiConverterBoolean.allocationSize(value.`confirmed`) +
            FfiConverterString.allocationSize(value.`suspicious0ConfReason`)
    )

    override fun write(value: IBtOnchainTransaction, buf: ByteBuffer) {
        FfiConverterULong.write(value.`amountSat`, buf)
        FfiConverterString.write(value.`txId`, buf)
        FfiConverterUInt.write(value.`vout`, buf)
        FfiConverterOptionalUInt.write(value.`blockHeight`, buf)
        FfiConverterUInt.write(value.`blockConfirmationCount`, buf)
        FfiConverterDouble.write(value.`feeRateSatPerVbyte`, buf)
        FfiConverterBoolean.write(value.`confirmed`, buf)
        FfiConverterString.write(value.`suspicious0ConfReason`, buf)
    }
}




public object FfiConverterTypeIBtOnchainTransactions: FfiConverterRustBuffer<IBtOnchainTransactions> {
    override fun read(buf: ByteBuffer): IBtOnchainTransactions {
        return IBtOnchainTransactions(
            FfiConverterString.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterSequenceTypeIBtOnchainTransaction.read(buf),
        )
    }

    override fun allocationSize(value: IBtOnchainTransactions): ULong = (
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterULong.allocationSize(value.`confirmedSat`) +
            FfiConverterUInt.allocationSize(value.`requiredConfirmations`) +
            FfiConverterSequenceTypeIBtOnchainTransaction.allocationSize(value.`transactions`)
    )

    override fun write(value: IBtOnchainTransactions, buf: ByteBuffer) {
        FfiConverterString.write(value.`address`, buf)
        FfiConverterULong.write(value.`confirmedSat`, buf)
        FfiConverterUInt.write(value.`requiredConfirmations`, buf)
        FfiConverterSequenceTypeIBtOnchainTransaction.write(value.`transactions`, buf)
    }
}




public object FfiConverterTypeIBtOrder: FfiConverterRustBuffer<IBtOrder> {
    override fun read(buf: ByteBuffer): IBtOrder {
        return IBtOrder(
            FfiConverterString.read(buf),
            FfiConverterTypeBtOrderState.read(buf),
            FfiConverterOptionalTypeBtOrderState2.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalTypeIBtChannel.read(buf),
            FfiConverterOptionalTypeILspNode.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalTypeIBtPayment.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalTypeIDiscount.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: IBtOrder): ULong = (
            FfiConverterString.allocationSize(value.`id`) +
            FfiConverterTypeBtOrderState.allocationSize(value.`state`) +
            FfiConverterOptionalTypeBtOrderState2.allocationSize(value.`state2`) +
            FfiConverterULong.allocationSize(value.`feeSat`) +
            FfiConverterULong.allocationSize(value.`networkFeeSat`) +
            FfiConverterULong.allocationSize(value.`serviceFeeSat`) +
            FfiConverterULong.allocationSize(value.`lspBalanceSat`) +
            FfiConverterULong.allocationSize(value.`clientBalanceSat`) +
            FfiConverterBoolean.allocationSize(value.`zeroConf`) +
            FfiConverterBoolean.allocationSize(value.`zeroReserve`) +
            FfiConverterOptionalString.allocationSize(value.`clientNodeId`) +
            FfiConverterUInt.allocationSize(value.`channelExpiryWeeks`) +
            FfiConverterString.allocationSize(value.`channelExpiresAt`) +
            FfiConverterString.allocationSize(value.`orderExpiresAt`) +
            FfiConverterOptionalTypeIBtChannel.allocationSize(value.`channel`) +
            FfiConverterOptionalTypeILspNode.allocationSize(value.`lspNode`) +
            FfiConverterOptionalString.allocationSize(value.`lnurl`) +
            FfiConverterOptionalTypeIBtPayment.allocationSize(value.`payment`) +
            FfiConverterOptionalString.allocationSize(value.`couponCode`) +
            FfiConverterOptionalString.allocationSize(value.`source`) +
            FfiConverterOptionalTypeIDiscount.allocationSize(value.`discount`) +
            FfiConverterString.allocationSize(value.`updatedAt`) +
            FfiConverterString.allocationSize(value.`createdAt`)
    )

    override fun write(value: IBtOrder, buf: ByteBuffer) {
        FfiConverterString.write(value.`id`, buf)
        FfiConverterTypeBtOrderState.write(value.`state`, buf)
        FfiConverterOptionalTypeBtOrderState2.write(value.`state2`, buf)
        FfiConverterULong.write(value.`feeSat`, buf)
        FfiConverterULong.write(value.`networkFeeSat`, buf)
        FfiConverterULong.write(value.`serviceFeeSat`, buf)
        FfiConverterULong.write(value.`lspBalanceSat`, buf)
        FfiConverterULong.write(value.`clientBalanceSat`, buf)
        FfiConverterBoolean.write(value.`zeroConf`, buf)
        FfiConverterBoolean.write(value.`zeroReserve`, buf)
        FfiConverterOptionalString.write(value.`clientNodeId`, buf)
        FfiConverterUInt.write(value.`channelExpiryWeeks`, buf)
        FfiConverterString.write(value.`channelExpiresAt`, buf)
        FfiConverterString.write(value.`orderExpiresAt`, buf)
        FfiConverterOptionalTypeIBtChannel.write(value.`channel`, buf)
        FfiConverterOptionalTypeILspNode.write(value.`lspNode`, buf)
        FfiConverterOptionalString.write(value.`lnurl`, buf)
        FfiConverterOptionalTypeIBtPayment.write(value.`payment`, buf)
        FfiConverterOptionalString.write(value.`couponCode`, buf)
        FfiConverterOptionalString.write(value.`source`, buf)
        FfiConverterOptionalTypeIDiscount.write(value.`discount`, buf)
        FfiConverterString.write(value.`updatedAt`, buf)
        FfiConverterString.write(value.`createdAt`, buf)
    }
}




public object FfiConverterTypeIBtPayment: FfiConverterRustBuffer<IBtPayment> {
    override fun read(buf: ByteBuffer): IBtPayment {
        return IBtPayment(
            FfiConverterTypeBtPaymentState.read(buf),
            FfiConverterOptionalTypeBtPaymentState2.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterOptionalTypeIBtBolt11Invoice.read(buf),
            FfiConverterOptionalTypeIBtOnchainTransactions.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalSequenceTypeIManualRefund.read(buf),
        )
    }

    override fun allocationSize(value: IBtPayment): ULong = (
            FfiConverterTypeBtPaymentState.allocationSize(value.`state`) +
            FfiConverterOptionalTypeBtPaymentState2.allocationSize(value.`state2`) +
            FfiConverterULong.allocationSize(value.`paidSat`) +
            FfiConverterOptionalTypeIBtBolt11Invoice.allocationSize(value.`bolt11Invoice`) +
            FfiConverterOptionalTypeIBtOnchainTransactions.allocationSize(value.`onchain`) +
            FfiConverterOptionalBoolean.allocationSize(value.`isManuallyPaid`) +
            FfiConverterOptionalSequenceTypeIManualRefund.allocationSize(value.`manualRefunds`)
    )

    override fun write(value: IBtPayment, buf: ByteBuffer) {
        FfiConverterTypeBtPaymentState.write(value.`state`, buf)
        FfiConverterOptionalTypeBtPaymentState2.write(value.`state2`, buf)
        FfiConverterULong.write(value.`paidSat`, buf)
        FfiConverterOptionalTypeIBtBolt11Invoice.write(value.`bolt11Invoice`, buf)
        FfiConverterOptionalTypeIBtOnchainTransactions.write(value.`onchain`, buf)
        FfiConverterOptionalBoolean.write(value.`isManuallyPaid`, buf)
        FfiConverterOptionalSequenceTypeIManualRefund.write(value.`manualRefunds`, buf)
    }
}




public object FfiConverterTypeICJitEntry: FfiConverterRustBuffer<IcJitEntry> {
    override fun read(buf: ByteBuffer): IcJitEntry {
        return IcJitEntry(
            FfiConverterString.read(buf),
            FfiConverterTypeCJitStateEnum.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterTypeIBtBolt11Invoice.read(buf),
            FfiConverterOptionalTypeIBtChannel.read(buf),
            FfiConverterTypeILspNode.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalTypeIDiscount.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: IcJitEntry): ULong = (
            FfiConverterString.allocationSize(value.`id`) +
            FfiConverterTypeCJitStateEnum.allocationSize(value.`state`) +
            FfiConverterULong.allocationSize(value.`feeSat`) +
            FfiConverterULong.allocationSize(value.`networkFeeSat`) +
            FfiConverterULong.allocationSize(value.`serviceFeeSat`) +
            FfiConverterULong.allocationSize(value.`channelSizeSat`) +
            FfiConverterUInt.allocationSize(value.`channelExpiryWeeks`) +
            FfiConverterOptionalString.allocationSize(value.`channelOpenError`) +
            FfiConverterString.allocationSize(value.`nodeId`) +
            FfiConverterTypeIBtBolt11Invoice.allocationSize(value.`invoice`) +
            FfiConverterOptionalTypeIBtChannel.allocationSize(value.`channel`) +
            FfiConverterTypeILspNode.allocationSize(value.`lspNode`) +
            FfiConverterString.allocationSize(value.`couponCode`) +
            FfiConverterOptionalString.allocationSize(value.`source`) +
            FfiConverterOptionalTypeIDiscount.allocationSize(value.`discount`) +
            FfiConverterString.allocationSize(value.`expiresAt`) +
            FfiConverterString.allocationSize(value.`updatedAt`) +
            FfiConverterString.allocationSize(value.`createdAt`)
    )

    override fun write(value: IcJitEntry, buf: ByteBuffer) {
        FfiConverterString.write(value.`id`, buf)
        FfiConverterTypeCJitStateEnum.write(value.`state`, buf)
        FfiConverterULong.write(value.`feeSat`, buf)
        FfiConverterULong.write(value.`networkFeeSat`, buf)
        FfiConverterULong.write(value.`serviceFeeSat`, buf)
        FfiConverterULong.write(value.`channelSizeSat`, buf)
        FfiConverterUInt.write(value.`channelExpiryWeeks`, buf)
        FfiConverterOptionalString.write(value.`channelOpenError`, buf)
        FfiConverterString.write(value.`nodeId`, buf)
        FfiConverterTypeIBtBolt11Invoice.write(value.`invoice`, buf)
        FfiConverterOptionalTypeIBtChannel.write(value.`channel`, buf)
        FfiConverterTypeILspNode.write(value.`lspNode`, buf)
        FfiConverterString.write(value.`couponCode`, buf)
        FfiConverterOptionalString.write(value.`source`, buf)
        FfiConverterOptionalTypeIDiscount.write(value.`discount`, buf)
        FfiConverterString.write(value.`expiresAt`, buf)
        FfiConverterString.write(value.`updatedAt`, buf)
        FfiConverterString.write(value.`createdAt`, buf)
    }
}




public object FfiConverterTypeIDiscount: FfiConverterRustBuffer<IDiscount> {
    override fun read(buf: ByteBuffer): IDiscount {
        return IDiscount(
            FfiConverterString.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterDouble.read(buf),
            FfiConverterULong.read(buf),
        )
    }

    override fun allocationSize(value: IDiscount): ULong = (
            FfiConverterString.allocationSize(value.`code`) +
            FfiConverterULong.allocationSize(value.`absoluteSat`) +
            FfiConverterDouble.allocationSize(value.`relative`) +
            FfiConverterULong.allocationSize(value.`overallSat`)
    )

    override fun write(value: IDiscount, buf: ByteBuffer) {
        FfiConverterString.write(value.`code`, buf)
        FfiConverterULong.write(value.`absoluteSat`, buf)
        FfiConverterDouble.write(value.`relative`, buf)
        FfiConverterULong.write(value.`overallSat`, buf)
    }
}




public object FfiConverterTypeIGift: FfiConverterRustBuffer<IGift> {
    override fun read(buf: ByteBuffer): IGift {
        return IGift(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalTypeIGiftOrder.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalTypeIGiftPayment.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalTypeIGiftCode.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
        )
    }

    override fun allocationSize(value: IGift): ULong = (
            FfiConverterString.allocationSize(value.`id`) +
            FfiConverterString.allocationSize(value.`nodeId`) +
            FfiConverterOptionalString.allocationSize(value.`orderId`) +
            FfiConverterOptionalTypeIGiftOrder.allocationSize(value.`order`) +
            FfiConverterOptionalString.allocationSize(value.`bolt11PaymentId`) +
            FfiConverterOptionalTypeIGiftPayment.allocationSize(value.`bolt11Payment`) +
            FfiConverterOptionalString.allocationSize(value.`appliedGiftCodeId`) +
            FfiConverterOptionalTypeIGiftCode.allocationSize(value.`appliedGiftCode`) +
            FfiConverterOptionalString.allocationSize(value.`createdAt`) +
            FfiConverterOptionalString.allocationSize(value.`updatedAt`)
    )

    override fun write(value: IGift, buf: ByteBuffer) {
        FfiConverterString.write(value.`id`, buf)
        FfiConverterString.write(value.`nodeId`, buf)
        FfiConverterOptionalString.write(value.`orderId`, buf)
        FfiConverterOptionalTypeIGiftOrder.write(value.`order`, buf)
        FfiConverterOptionalString.write(value.`bolt11PaymentId`, buf)
        FfiConverterOptionalTypeIGiftPayment.write(value.`bolt11Payment`, buf)
        FfiConverterOptionalString.write(value.`appliedGiftCodeId`, buf)
        FfiConverterOptionalTypeIGiftCode.write(value.`appliedGiftCode`, buf)
        FfiConverterOptionalString.write(value.`createdAt`, buf)
        FfiConverterOptionalString.write(value.`updatedAt`, buf)
    }
}




public object FfiConverterTypeIGiftBolt11Invoice: FfiConverterRustBuffer<IGiftBolt11Invoice> {
    override fun read(buf: ByteBuffer): IGiftBolt11Invoice {
        return IGiftBolt11Invoice(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
        )
    }

    override fun allocationSize(value: IGiftBolt11Invoice): ULong = (
            FfiConverterString.allocationSize(value.`id`) +
            FfiConverterString.allocationSize(value.`request`) +
            FfiConverterString.allocationSize(value.`state`) +
            FfiConverterOptionalBoolean.allocationSize(value.`isHodlInvoice`) +
            FfiConverterOptionalString.allocationSize(value.`paymentHash`) +
            FfiConverterOptionalULong.allocationSize(value.`amountSat`) +
            FfiConverterOptionalString.allocationSize(value.`amountMsat`) +
            FfiConverterOptionalString.allocationSize(value.`internalNodePubkey`) +
            FfiConverterOptionalString.allocationSize(value.`updatedAt`) +
            FfiConverterOptionalString.allocationSize(value.`createdAt`) +
            FfiConverterOptionalString.allocationSize(value.`expiresAt`)
    )

    override fun write(value: IGiftBolt11Invoice, buf: ByteBuffer) {
        FfiConverterString.write(value.`id`, buf)
        FfiConverterString.write(value.`request`, buf)
        FfiConverterString.write(value.`state`, buf)
        FfiConverterOptionalBoolean.write(value.`isHodlInvoice`, buf)
        FfiConverterOptionalString.write(value.`paymentHash`, buf)
        FfiConverterOptionalULong.write(value.`amountSat`, buf)
        FfiConverterOptionalString.write(value.`amountMsat`, buf)
        FfiConverterOptionalString.write(value.`internalNodePubkey`, buf)
        FfiConverterOptionalString.write(value.`updatedAt`, buf)
        FfiConverterOptionalString.write(value.`createdAt`, buf)
        FfiConverterOptionalString.write(value.`expiresAt`, buf)
    }
}




public object FfiConverterTypeIGiftBtcAddress: FfiConverterRustBuffer<IGiftBtcAddress> {
    override fun read(buf: ByteBuffer): IGiftBtcAddress {
        return IGiftBtcAddress(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterSequenceString.read(buf),
            FfiConverterSequenceString.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
        )
    }

    override fun allocationSize(value: IGiftBtcAddress): ULong = (
            FfiConverterString.allocationSize(value.`id`) +
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterSequenceString.allocationSize(value.`transactions`) +
            FfiConverterSequenceString.allocationSize(value.`allTransactions`) +
            FfiConverterOptionalBoolean.allocationSize(value.`isBlacklisted`) +
            FfiConverterOptionalString.allocationSize(value.`watchUntil`) +
            FfiConverterOptionalUInt.allocationSize(value.`watchForBlockConfirmations`) +
            FfiConverterOptionalString.allocationSize(value.`updatedAt`) +
            FfiConverterOptionalString.allocationSize(value.`createdAt`)
    )

    override fun write(value: IGiftBtcAddress, buf: ByteBuffer) {
        FfiConverterString.write(value.`id`, buf)
        FfiConverterString.write(value.`address`, buf)
        FfiConverterSequenceString.write(value.`transactions`, buf)
        FfiConverterSequenceString.write(value.`allTransactions`, buf)
        FfiConverterOptionalBoolean.write(value.`isBlacklisted`, buf)
        FfiConverterOptionalString.write(value.`watchUntil`, buf)
        FfiConverterOptionalUInt.write(value.`watchForBlockConfirmations`, buf)
        FfiConverterOptionalString.write(value.`updatedAt`, buf)
        FfiConverterOptionalString.write(value.`createdAt`, buf)
    }
}




public object FfiConverterTypeIGiftCode: FfiConverterRustBuffer<IGiftCode> {
    override fun read(buf: ByteBuffer): IGiftCode {
        return IGiftCode(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalUInt.read(buf),
        )
    }

    override fun allocationSize(value: IGiftCode): ULong = (
            FfiConverterString.allocationSize(value.`id`) +
            FfiConverterString.allocationSize(value.`code`) +
            FfiConverterString.allocationSize(value.`createdAt`) +
            FfiConverterString.allocationSize(value.`updatedAt`) +
            FfiConverterString.allocationSize(value.`expiresAt`) +
            FfiConverterOptionalULong.allocationSize(value.`giftSat`) +
            FfiConverterOptionalString.allocationSize(value.`scope`) +
            FfiConverterOptionalUInt.allocationSize(value.`maxCount`)
    )

    override fun write(value: IGiftCode, buf: ByteBuffer) {
        FfiConverterString.write(value.`id`, buf)
        FfiConverterString.write(value.`code`, buf)
        FfiConverterString.write(value.`createdAt`, buf)
        FfiConverterString.write(value.`updatedAt`, buf)
        FfiConverterString.write(value.`expiresAt`, buf)
        FfiConverterOptionalULong.write(value.`giftSat`, buf)
        FfiConverterOptionalString.write(value.`scope`, buf)
        FfiConverterOptionalUInt.write(value.`maxCount`, buf)
    }
}




public object FfiConverterTypeIGiftLspNode: FfiConverterRustBuffer<IGiftLspNode> {
    override fun read(buf: ByteBuffer): IGiftLspNode {
        return IGiftLspNode(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterSequenceString.read(buf),
        )
    }

    override fun allocationSize(value: IGiftLspNode): ULong = (
            FfiConverterString.allocationSize(value.`alias`) +
            FfiConverterString.allocationSize(value.`pubkey`) +
            FfiConverterSequenceString.allocationSize(value.`connectionStrings`)
    )

    override fun write(value: IGiftLspNode, buf: ByteBuffer) {
        FfiConverterString.write(value.`alias`, buf)
        FfiConverterString.write(value.`pubkey`, buf)
        FfiConverterSequenceString.write(value.`connectionStrings`, buf)
    }
}




public object FfiConverterTypeIGiftOrder: FfiConverterRustBuffer<IGiftOrder> {
    override fun read(buf: ByteBuffer): IGiftOrder {
        return IGiftOrder(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalTypeIGiftPayment.read(buf),
            FfiConverterOptionalTypeIGiftLspNode.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalBoolean.read(buf),
        )
    }

    override fun allocationSize(value: IGiftOrder): ULong = (
            FfiConverterString.allocationSize(value.`id`) +
            FfiConverterString.allocationSize(value.`state`) +
            FfiConverterOptionalString.allocationSize(value.`oldState`) +
            FfiConverterOptionalBoolean.allocationSize(value.`isChannelExpired`) +
            FfiConverterOptionalBoolean.allocationSize(value.`isOrderExpired`) +
            FfiConverterOptionalULong.allocationSize(value.`lspBalanceSat`) +
            FfiConverterOptionalULong.allocationSize(value.`clientBalanceSat`) +
            FfiConverterOptionalUInt.allocationSize(value.`channelExpiryWeeks`) +
            FfiConverterOptionalBoolean.allocationSize(value.`zeroConf`) +
            FfiConverterOptionalBoolean.allocationSize(value.`zeroReserve`) +
            FfiConverterOptionalBoolean.allocationSize(value.`announced`) +
            FfiConverterOptionalString.allocationSize(value.`clientNodeId`) +
            FfiConverterOptionalString.allocationSize(value.`channelExpiresAt`) +
            FfiConverterOptionalString.allocationSize(value.`orderExpiresAt`) +
            FfiConverterOptionalULong.allocationSize(value.`feeSat`) +
            FfiConverterOptionalULong.allocationSize(value.`networkFeeSat`) +
            FfiConverterOptionalULong.allocationSize(value.`serviceFeeSat`) +
            FfiConverterOptionalTypeIGiftPayment.allocationSize(value.`payment`) +
            FfiConverterOptionalTypeIGiftLspNode.allocationSize(value.`lspNode`) +
            FfiConverterOptionalString.allocationSize(value.`updatedAt`) +
            FfiConverterOptionalString.allocationSize(value.`createdAt`) +
            FfiConverterOptionalBoolean.allocationSize(value.`nodeIdVerified`)
    )

    override fun write(value: IGiftOrder, buf: ByteBuffer) {
        FfiConverterString.write(value.`id`, buf)
        FfiConverterString.write(value.`state`, buf)
        FfiConverterOptionalString.write(value.`oldState`, buf)
        FfiConverterOptionalBoolean.write(value.`isChannelExpired`, buf)
        FfiConverterOptionalBoolean.write(value.`isOrderExpired`, buf)
        FfiConverterOptionalULong.write(value.`lspBalanceSat`, buf)
        FfiConverterOptionalULong.write(value.`clientBalanceSat`, buf)
        FfiConverterOptionalUInt.write(value.`channelExpiryWeeks`, buf)
        FfiConverterOptionalBoolean.write(value.`zeroConf`, buf)
        FfiConverterOptionalBoolean.write(value.`zeroReserve`, buf)
        FfiConverterOptionalBoolean.write(value.`announced`, buf)
        FfiConverterOptionalString.write(value.`clientNodeId`, buf)
        FfiConverterOptionalString.write(value.`channelExpiresAt`, buf)
        FfiConverterOptionalString.write(value.`orderExpiresAt`, buf)
        FfiConverterOptionalULong.write(value.`feeSat`, buf)
        FfiConverterOptionalULong.write(value.`networkFeeSat`, buf)
        FfiConverterOptionalULong.write(value.`serviceFeeSat`, buf)
        FfiConverterOptionalTypeIGiftPayment.write(value.`payment`, buf)
        FfiConverterOptionalTypeIGiftLspNode.write(value.`lspNode`, buf)
        FfiConverterOptionalString.write(value.`updatedAt`, buf)
        FfiConverterOptionalString.write(value.`createdAt`, buf)
        FfiConverterOptionalBoolean.write(value.`nodeIdVerified`, buf)
    }
}




public object FfiConverterTypeIGiftPayment: FfiConverterRustBuffer<IGiftPayment> {
    override fun read(buf: ByteBuffer): IGiftPayment {
        return IGiftPayment(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalTypeIGiftBtcAddress.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalTypeIGiftBolt11Invoice.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterSequenceString.read(buf),
        )
    }

    override fun allocationSize(value: IGiftPayment): ULong = (
            FfiConverterString.allocationSize(value.`id`) +
            FfiConverterString.allocationSize(value.`state`) +
            FfiConverterOptionalString.allocationSize(value.`oldState`) +
            FfiConverterOptionalString.allocationSize(value.`onchainState`) +
            FfiConverterOptionalString.allocationSize(value.`lnState`) +
            FfiConverterOptionalULong.allocationSize(value.`paidOnchainSat`) +
            FfiConverterOptionalULong.allocationSize(value.`paidLnSat`) +
            FfiConverterOptionalULong.allocationSize(value.`paidSat`) +
            FfiConverterOptionalBoolean.allocationSize(value.`isOverpaid`) +
            FfiConverterOptionalBoolean.allocationSize(value.`isRefunded`) +
            FfiConverterOptionalULong.allocationSize(value.`overpaidAmountSat`) +
            FfiConverterOptionalUInt.allocationSize(value.`requiredOnchainConfirmations`) +
            FfiConverterOptionalString.allocationSize(value.`settlementState`) +
            FfiConverterOptionalULong.allocationSize(value.`expectedAmountSat`) +
            FfiConverterOptionalBoolean.allocationSize(value.`isManuallyPaid`) +
            FfiConverterOptionalTypeIGiftBtcAddress.allocationSize(value.`btcAddress`) +
            FfiConverterOptionalString.allocationSize(value.`btcAddressId`) +
            FfiConverterOptionalTypeIGiftBolt11Invoice.allocationSize(value.`bolt11Invoice`) +
            FfiConverterOptionalString.allocationSize(value.`bolt11InvoiceId`) +
            FfiConverterSequenceString.allocationSize(value.`manualRefunds`)
    )

    override fun write(value: IGiftPayment, buf: ByteBuffer) {
        FfiConverterString.write(value.`id`, buf)
        FfiConverterString.write(value.`state`, buf)
        FfiConverterOptionalString.write(value.`oldState`, buf)
        FfiConverterOptionalString.write(value.`onchainState`, buf)
        FfiConverterOptionalString.write(value.`lnState`, buf)
        FfiConverterOptionalULong.write(value.`paidOnchainSat`, buf)
        FfiConverterOptionalULong.write(value.`paidLnSat`, buf)
        FfiConverterOptionalULong.write(value.`paidSat`, buf)
        FfiConverterOptionalBoolean.write(value.`isOverpaid`, buf)
        FfiConverterOptionalBoolean.write(value.`isRefunded`, buf)
        FfiConverterOptionalULong.write(value.`overpaidAmountSat`, buf)
        FfiConverterOptionalUInt.write(value.`requiredOnchainConfirmations`, buf)
        FfiConverterOptionalString.write(value.`settlementState`, buf)
        FfiConverterOptionalULong.write(value.`expectedAmountSat`, buf)
        FfiConverterOptionalBoolean.write(value.`isManuallyPaid`, buf)
        FfiConverterOptionalTypeIGiftBtcAddress.write(value.`btcAddress`, buf)
        FfiConverterOptionalString.write(value.`btcAddressId`, buf)
        FfiConverterOptionalTypeIGiftBolt11Invoice.write(value.`bolt11Invoice`, buf)
        FfiConverterOptionalString.write(value.`bolt11InvoiceId`, buf)
        FfiConverterSequenceString.write(value.`manualRefunds`, buf)
    }
}




public object FfiConverterTypeILspNode: FfiConverterRustBuffer<ILspNode> {
    override fun read(buf: ByteBuffer): ILspNode {
        return ILspNode(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterSequenceString.read(buf),
            FfiConverterOptionalBoolean.read(buf),
        )
    }

    override fun allocationSize(value: ILspNode): ULong = (
            FfiConverterString.allocationSize(value.`alias`) +
            FfiConverterString.allocationSize(value.`pubkey`) +
            FfiConverterSequenceString.allocationSize(value.`connectionStrings`) +
            FfiConverterOptionalBoolean.allocationSize(value.`readonly`)
    )

    override fun write(value: ILspNode, buf: ByteBuffer) {
        FfiConverterString.write(value.`alias`, buf)
        FfiConverterString.write(value.`pubkey`, buf)
        FfiConverterSequenceString.write(value.`connectionStrings`, buf)
        FfiConverterOptionalBoolean.write(value.`readonly`, buf)
    }
}




public object FfiConverterTypeIManualRefund: FfiConverterRustBuffer<IManualRefund> {
    override fun read(buf: ByteBuffer): IManualRefund {
        return IManualRefund(
            FfiConverterULong.read(buf),
            FfiConverterString.read(buf),
            FfiConverterTypeManualRefundStateEnum.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: IManualRefund): ULong = (
            FfiConverterULong.allocationSize(value.`amountSat`) +
            FfiConverterString.allocationSize(value.`target`) +
            FfiConverterTypeManualRefundStateEnum.allocationSize(value.`state`) +
            FfiConverterString.allocationSize(value.`createdByName`) +
            FfiConverterOptionalString.allocationSize(value.`votedByName`) +
            FfiConverterOptionalString.allocationSize(value.`reason`) +
            FfiConverterString.allocationSize(value.`targetType`)
    )

    override fun write(value: IManualRefund, buf: ByteBuffer) {
        FfiConverterULong.write(value.`amountSat`, buf)
        FfiConverterString.write(value.`target`, buf)
        FfiConverterTypeManualRefundStateEnum.write(value.`state`, buf)
        FfiConverterString.write(value.`createdByName`, buf)
        FfiConverterOptionalString.write(value.`votedByName`, buf)
        FfiConverterOptionalString.write(value.`reason`, buf)
        FfiConverterString.write(value.`targetType`, buf)
    }
}




public object FfiConverterTypeLightningActivity: FfiConverterRustBuffer<LightningActivity> {
    override fun read(buf: ByteBuffer): LightningActivity {
        return LightningActivity(
            FfiConverterString.read(buf),
            FfiConverterTypePaymentType.read(buf),
            FfiConverterTypePaymentState.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalULong.read(buf),
        )
    }

    override fun allocationSize(value: LightningActivity): ULong = (
            FfiConverterString.allocationSize(value.`id`) +
            FfiConverterTypePaymentType.allocationSize(value.`txType`) +
            FfiConverterTypePaymentState.allocationSize(value.`status`) +
            FfiConverterULong.allocationSize(value.`value`) +
            FfiConverterOptionalULong.allocationSize(value.`fee`) +
            FfiConverterString.allocationSize(value.`invoice`) +
            FfiConverterString.allocationSize(value.`message`) +
            FfiConverterULong.allocationSize(value.`timestamp`) +
            FfiConverterOptionalString.allocationSize(value.`preimage`) +
            FfiConverterOptionalULong.allocationSize(value.`createdAt`) +
            FfiConverterOptionalULong.allocationSize(value.`updatedAt`)
    )

    override fun write(value: LightningActivity, buf: ByteBuffer) {
        FfiConverterString.write(value.`id`, buf)
        FfiConverterTypePaymentType.write(value.`txType`, buf)
        FfiConverterTypePaymentState.write(value.`status`, buf)
        FfiConverterULong.write(value.`value`, buf)
        FfiConverterOptionalULong.write(value.`fee`, buf)
        FfiConverterString.write(value.`invoice`, buf)
        FfiConverterString.write(value.`message`, buf)
        FfiConverterULong.write(value.`timestamp`, buf)
        FfiConverterOptionalString.write(value.`preimage`, buf)
        FfiConverterOptionalULong.write(value.`createdAt`, buf)
        FfiConverterOptionalULong.write(value.`updatedAt`, buf)
    }
}




public object FfiConverterTypeLightningInvoice: FfiConverterRustBuffer<LightningInvoice> {
    override fun read(buf: ByteBuffer): LightningInvoice {
        return LightningInvoice(
            FfiConverterString.read(buf),
            FfiConverterByteArray.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterTypeNetworkType.read(buf),
            FfiConverterOptionalByteArray.read(buf),
        )
    }

    override fun allocationSize(value: LightningInvoice): ULong = (
            FfiConverterString.allocationSize(value.`bolt11`) +
            FfiConverterByteArray.allocationSize(value.`paymentHash`) +
            FfiConverterULong.allocationSize(value.`amountSatoshis`) +
            FfiConverterULong.allocationSize(value.`timestampSeconds`) +
            FfiConverterULong.allocationSize(value.`expirySeconds`) +
            FfiConverterBoolean.allocationSize(value.`isExpired`) +
            FfiConverterOptionalString.allocationSize(value.`description`) +
            FfiConverterTypeNetworkType.allocationSize(value.`networkType`) +
            FfiConverterOptionalByteArray.allocationSize(value.`payeeNodeId`)
    )

    override fun write(value: LightningInvoice, buf: ByteBuffer) {
        FfiConverterString.write(value.`bolt11`, buf)
        FfiConverterByteArray.write(value.`paymentHash`, buf)
        FfiConverterULong.write(value.`amountSatoshis`, buf)
        FfiConverterULong.write(value.`timestampSeconds`, buf)
        FfiConverterULong.write(value.`expirySeconds`, buf)
        FfiConverterBoolean.write(value.`isExpired`, buf)
        FfiConverterOptionalString.write(value.`description`, buf)
        FfiConverterTypeNetworkType.write(value.`networkType`, buf)
        FfiConverterOptionalByteArray.write(value.`payeeNodeId`, buf)
    }
}




public object FfiConverterTypeLnurlAddressData: FfiConverterRustBuffer<LnurlAddressData> {
    override fun read(buf: ByteBuffer): LnurlAddressData {
        return LnurlAddressData(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: LnurlAddressData): ULong = (
            FfiConverterString.allocationSize(value.`uri`) +
            FfiConverterString.allocationSize(value.`domain`) +
            FfiConverterString.allocationSize(value.`username`)
    )

    override fun write(value: LnurlAddressData, buf: ByteBuffer) {
        FfiConverterString.write(value.`uri`, buf)
        FfiConverterString.write(value.`domain`, buf)
        FfiConverterString.write(value.`username`, buf)
    }
}




public object FfiConverterTypeLnurlAuthData: FfiConverterRustBuffer<LnurlAuthData> {
    override fun read(buf: ByteBuffer): LnurlAuthData {
        return LnurlAuthData(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: LnurlAuthData): ULong = (
            FfiConverterString.allocationSize(value.`uri`) +
            FfiConverterString.allocationSize(value.`tag`) +
            FfiConverterString.allocationSize(value.`k1`) +
            FfiConverterString.allocationSize(value.`domain`)
    )

    override fun write(value: LnurlAuthData, buf: ByteBuffer) {
        FfiConverterString.write(value.`uri`, buf)
        FfiConverterString.write(value.`tag`, buf)
        FfiConverterString.write(value.`k1`, buf)
        FfiConverterString.write(value.`domain`, buf)
    }
}




public object FfiConverterTypeLnurlChannelData: FfiConverterRustBuffer<LnurlChannelData> {
    override fun read(buf: ByteBuffer): LnurlChannelData {
        return LnurlChannelData(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: LnurlChannelData): ULong = (
            FfiConverterString.allocationSize(value.`uri`) +
            FfiConverterString.allocationSize(value.`callback`) +
            FfiConverterString.allocationSize(value.`k1`) +
            FfiConverterString.allocationSize(value.`tag`)
    )

    override fun write(value: LnurlChannelData, buf: ByteBuffer) {
        FfiConverterString.write(value.`uri`, buf)
        FfiConverterString.write(value.`callback`, buf)
        FfiConverterString.write(value.`k1`, buf)
        FfiConverterString.write(value.`tag`, buf)
    }
}




public object FfiConverterTypeLnurlPayData: FfiConverterRustBuffer<LnurlPayData> {
    override fun read(buf: ByteBuffer): LnurlPayData {
        return LnurlPayData(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterOptionalByteArray.read(buf),
        )
    }

    override fun allocationSize(value: LnurlPayData): ULong = (
            FfiConverterString.allocationSize(value.`uri`) +
            FfiConverterString.allocationSize(value.`callback`) +
            FfiConverterULong.allocationSize(value.`minSendable`) +
            FfiConverterULong.allocationSize(value.`maxSendable`) +
            FfiConverterString.allocationSize(value.`metadataStr`) +
            FfiConverterOptionalUInt.allocationSize(value.`commentAllowed`) +
            FfiConverterBoolean.allocationSize(value.`allowsNostr`) +
            FfiConverterOptionalByteArray.allocationSize(value.`nostrPubkey`)
    )

    override fun write(value: LnurlPayData, buf: ByteBuffer) {
        FfiConverterString.write(value.`uri`, buf)
        FfiConverterString.write(value.`callback`, buf)
        FfiConverterULong.write(value.`minSendable`, buf)
        FfiConverterULong.write(value.`maxSendable`, buf)
        FfiConverterString.write(value.`metadataStr`, buf)
        FfiConverterOptionalUInt.write(value.`commentAllowed`, buf)
        FfiConverterBoolean.write(value.`allowsNostr`, buf)
        FfiConverterOptionalByteArray.write(value.`nostrPubkey`, buf)
    }
}




public object FfiConverterTypeLnurlWithdrawData: FfiConverterRustBuffer<LnurlWithdrawData> {
    override fun read(buf: ByteBuffer): LnurlWithdrawData {
        return LnurlWithdrawData(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: LnurlWithdrawData): ULong = (
            FfiConverterString.allocationSize(value.`uri`) +
            FfiConverterString.allocationSize(value.`callback`) +
            FfiConverterString.allocationSize(value.`k1`) +
            FfiConverterString.allocationSize(value.`defaultDescription`) +
            FfiConverterOptionalULong.allocationSize(value.`minWithdrawable`) +
            FfiConverterULong.allocationSize(value.`maxWithdrawable`) +
            FfiConverterString.allocationSize(value.`tag`)
    )

    override fun write(value: LnurlWithdrawData, buf: ByteBuffer) {
        FfiConverterString.write(value.`uri`, buf)
        FfiConverterString.write(value.`callback`, buf)
        FfiConverterString.write(value.`k1`, buf)
        FfiConverterString.write(value.`defaultDescription`, buf)
        FfiConverterOptionalULong.write(value.`minWithdrawable`, buf)
        FfiConverterULong.write(value.`maxWithdrawable`, buf)
        FfiConverterString.write(value.`tag`, buf)
    }
}




public object FfiConverterTypeMessageSignatureResponse: FfiConverterRustBuffer<MessageSignatureResponse> {
    override fun read(buf: ByteBuffer): MessageSignatureResponse {
        return MessageSignatureResponse(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: MessageSignatureResponse): ULong = (
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterString.allocationSize(value.`signature`)
    )

    override fun write(value: MessageSignatureResponse, buf: ByteBuffer) {
        FfiConverterString.write(value.`address`, buf)
        FfiConverterString.write(value.`signature`, buf)
    }
}




public object FfiConverterTypeMultisigRedeemScriptType: FfiConverterRustBuffer<MultisigRedeemScriptType> {
    override fun read(buf: ByteBuffer): MultisigRedeemScriptType {
        return MultisigRedeemScriptType(
            FfiConverterSequenceTypeHDNodePathType.read(buf),
            FfiConverterSequenceString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterOptionalSequenceTypeHDNodeType.read(buf),
            FfiConverterOptionalUByte.read(buf),
        )
    }

    override fun allocationSize(value: MultisigRedeemScriptType): ULong = (
            FfiConverterSequenceTypeHDNodePathType.allocationSize(value.`pubkeys`) +
            FfiConverterSequenceString.allocationSize(value.`signatures`) +
            FfiConverterUInt.allocationSize(value.`m`) +
            FfiConverterOptionalSequenceTypeHDNodeType.allocationSize(value.`nodes`) +
            FfiConverterOptionalUByte.allocationSize(value.`pubkeysOrder`)
    )

    override fun write(value: MultisigRedeemScriptType, buf: ByteBuffer) {
        FfiConverterSequenceTypeHDNodePathType.write(value.`pubkeys`, buf)
        FfiConverterSequenceString.write(value.`signatures`, buf)
        FfiConverterUInt.write(value.`m`, buf)
        FfiConverterOptionalSequenceTypeHDNodeType.write(value.`nodes`, buf)
        FfiConverterOptionalUByte.write(value.`pubkeysOrder`, buf)
    }
}




public object FfiConverterTypeOnChainInvoice: FfiConverterRustBuffer<OnChainInvoice> {
    override fun read(buf: ByteBuffer): OnChainInvoice {
        return OnChainInvoice(
            FfiConverterString.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalMapStringString.read(buf),
        )
    }

    override fun allocationSize(value: OnChainInvoice): ULong = (
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterULong.allocationSize(value.`amountSatoshis`) +
            FfiConverterOptionalString.allocationSize(value.`label`) +
            FfiConverterOptionalString.allocationSize(value.`message`) +
            FfiConverterOptionalMapStringString.allocationSize(value.`params`)
    )

    override fun write(value: OnChainInvoice, buf: ByteBuffer) {
        FfiConverterString.write(value.`address`, buf)
        FfiConverterULong.write(value.`amountSatoshis`, buf)
        FfiConverterOptionalString.write(value.`label`, buf)
        FfiConverterOptionalString.write(value.`message`, buf)
        FfiConverterOptionalMapStringString.write(value.`params`, buf)
    }
}




public object FfiConverterTypeOnchainActivity: FfiConverterRustBuffer<OnchainActivity> {
    override fun read(buf: ByteBuffer): OnchainActivity {
        return OnchainActivity(
            FfiConverterString.read(buf),
            FfiConverterTypePaymentType.read(buf),
            FfiConverterString.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterString.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterSequenceString.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterOptionalULong.read(buf),
        )
    }

    override fun allocationSize(value: OnchainActivity): ULong = (
            FfiConverterString.allocationSize(value.`id`) +
            FfiConverterTypePaymentType.allocationSize(value.`txType`) +
            FfiConverterString.allocationSize(value.`txId`) +
            FfiConverterULong.allocationSize(value.`value`) +
            FfiConverterULong.allocationSize(value.`fee`) +
            FfiConverterULong.allocationSize(value.`feeRate`) +
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterBoolean.allocationSize(value.`confirmed`) +
            FfiConverterULong.allocationSize(value.`timestamp`) +
            FfiConverterBoolean.allocationSize(value.`isBoosted`) +
            FfiConverterSequenceString.allocationSize(value.`boostTxIds`) +
            FfiConverterBoolean.allocationSize(value.`isTransfer`) +
            FfiConverterBoolean.allocationSize(value.`doesExist`) +
            FfiConverterOptionalULong.allocationSize(value.`confirmTimestamp`) +
            FfiConverterOptionalString.allocationSize(value.`channelId`) +
            FfiConverterOptionalString.allocationSize(value.`transferTxId`) +
            FfiConverterOptionalULong.allocationSize(value.`createdAt`) +
            FfiConverterOptionalULong.allocationSize(value.`updatedAt`)
    )

    override fun write(value: OnchainActivity, buf: ByteBuffer) {
        FfiConverterString.write(value.`id`, buf)
        FfiConverterTypePaymentType.write(value.`txType`, buf)
        FfiConverterString.write(value.`txId`, buf)
        FfiConverterULong.write(value.`value`, buf)
        FfiConverterULong.write(value.`fee`, buf)
        FfiConverterULong.write(value.`feeRate`, buf)
        FfiConverterString.write(value.`address`, buf)
        FfiConverterBoolean.write(value.`confirmed`, buf)
        FfiConverterULong.write(value.`timestamp`, buf)
        FfiConverterBoolean.write(value.`isBoosted`, buf)
        FfiConverterSequenceString.write(value.`boostTxIds`, buf)
        FfiConverterBoolean.write(value.`isTransfer`, buf)
        FfiConverterBoolean.write(value.`doesExist`, buf)
        FfiConverterOptionalULong.write(value.`confirmTimestamp`, buf)
        FfiConverterOptionalString.write(value.`channelId`, buf)
        FfiConverterOptionalString.write(value.`transferTxId`, buf)
        FfiConverterOptionalULong.write(value.`createdAt`, buf)
        FfiConverterOptionalULong.write(value.`updatedAt`, buf)
    }
}




public object FfiConverterTypePaymentRequestMemo: FfiConverterRustBuffer<PaymentRequestMemo> {
    override fun read(buf: ByteBuffer): PaymentRequestMemo {
        return PaymentRequestMemo(
            FfiConverterOptionalTypeTextMemo.read(buf),
            FfiConverterOptionalTypeRefundMemo.read(buf),
            FfiConverterOptionalTypeCoinPurchaseMemo.read(buf),
        )
    }

    override fun allocationSize(value: PaymentRequestMemo): ULong = (
            FfiConverterOptionalTypeTextMemo.allocationSize(value.`textMemo`) +
            FfiConverterOptionalTypeRefundMemo.allocationSize(value.`refundMemo`) +
            FfiConverterOptionalTypeCoinPurchaseMemo.allocationSize(value.`coinPurchaseMemo`)
    )

    override fun write(value: PaymentRequestMemo, buf: ByteBuffer) {
        FfiConverterOptionalTypeTextMemo.write(value.`textMemo`, buf)
        FfiConverterOptionalTypeRefundMemo.write(value.`refundMemo`, buf)
        FfiConverterOptionalTypeCoinPurchaseMemo.write(value.`coinPurchaseMemo`, buf)
    }
}




public object FfiConverterTypePreActivityMetadata: FfiConverterRustBuffer<PreActivityMetadata> {
    override fun read(buf: ByteBuffer): PreActivityMetadata {
        return PreActivityMetadata(
            FfiConverterString.read(buf),
            FfiConverterSequenceString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterULong.read(buf),
        )
    }

    override fun allocationSize(value: PreActivityMetadata): ULong = (
            FfiConverterString.allocationSize(value.`paymentId`) +
            FfiConverterSequenceString.allocationSize(value.`tags`) +
            FfiConverterOptionalString.allocationSize(value.`paymentHash`) +
            FfiConverterOptionalString.allocationSize(value.`txId`) +
            FfiConverterOptionalString.allocationSize(value.`address`) +
            FfiConverterBoolean.allocationSize(value.`isReceive`) +
            FfiConverterULong.allocationSize(value.`feeRate`) +
            FfiConverterBoolean.allocationSize(value.`isTransfer`) +
            FfiConverterOptionalString.allocationSize(value.`channelId`) +
            FfiConverterULong.allocationSize(value.`createdAt`)
    )

    override fun write(value: PreActivityMetadata, buf: ByteBuffer) {
        FfiConverterString.write(value.`paymentId`, buf)
        FfiConverterSequenceString.write(value.`tags`, buf)
        FfiConverterOptionalString.write(value.`paymentHash`, buf)
        FfiConverterOptionalString.write(value.`txId`, buf)
        FfiConverterOptionalString.write(value.`address`, buf)
        FfiConverterBoolean.write(value.`isReceive`, buf)
        FfiConverterULong.write(value.`feeRate`, buf)
        FfiConverterBoolean.write(value.`isTransfer`, buf)
        FfiConverterOptionalString.write(value.`channelId`, buf)
        FfiConverterULong.write(value.`createdAt`, buf)
    }
}




public object FfiConverterTypePrecomposedInput: FfiConverterRustBuffer<PrecomposedInput> {
    override fun read(buf: ByteBuffer): PrecomposedInput {
        return PrecomposedInput(
            FfiConverterSequenceUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterTypeScriptType.read(buf),
        )
    }

    override fun allocationSize(value: PrecomposedInput): ULong = (
            FfiConverterSequenceUInt.allocationSize(value.`addressN`) +
            FfiConverterString.allocationSize(value.`amount`) +
            FfiConverterString.allocationSize(value.`prevHash`) +
            FfiConverterUInt.allocationSize(value.`prevIndex`) +
            FfiConverterTypeScriptType.allocationSize(value.`scriptType`)
    )

    override fun write(value: PrecomposedInput, buf: ByteBuffer) {
        FfiConverterSequenceUInt.write(value.`addressN`, buf)
        FfiConverterString.write(value.`amount`, buf)
        FfiConverterString.write(value.`prevHash`, buf)
        FfiConverterUInt.write(value.`prevIndex`, buf)
        FfiConverterTypeScriptType.write(value.`scriptType`, buf)
    }
}




public object FfiConverterTypePrecomposedOutput: FfiConverterRustBuffer<PrecomposedOutput> {
    override fun read(buf: ByteBuffer): PrecomposedOutput {
        return PrecomposedOutput(
            FfiConverterOptionalSequenceUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterTypeScriptType.read(buf),
        )
    }

    override fun allocationSize(value: PrecomposedOutput): ULong = (
            FfiConverterOptionalSequenceUInt.allocationSize(value.`addressN`) +
            FfiConverterString.allocationSize(value.`amount`) +
            FfiConverterOptionalString.allocationSize(value.`address`) +
            FfiConverterTypeScriptType.allocationSize(value.`scriptType`)
    )

    override fun write(value: PrecomposedOutput, buf: ByteBuffer) {
        FfiConverterOptionalSequenceUInt.write(value.`addressN`, buf)
        FfiConverterString.write(value.`amount`, buf)
        FfiConverterOptionalString.write(value.`address`, buf)
        FfiConverterTypeScriptType.write(value.`scriptType`, buf)
    }
}




public object FfiConverterTypePrecomposedTransaction: FfiConverterRustBuffer<PrecomposedTransaction> {
    override fun read(buf: ByteBuffer): PrecomposedTransaction {
        return PrecomposedTransaction(
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalSequenceTypePrecomposedInput.read(buf),
            FfiConverterOptionalSequenceTypePrecomposedOutput.read(buf),
            FfiConverterOptionalSequenceUInt.read(buf),
        )
    }

    override fun allocationSize(value: PrecomposedTransaction): ULong = (
            FfiConverterString.allocationSize(value.`txType`) +
            FfiConverterOptionalString.allocationSize(value.`totalSpent`) +
            FfiConverterOptionalString.allocationSize(value.`fee`) +
            FfiConverterOptionalString.allocationSize(value.`feePerByte`) +
            FfiConverterOptionalUInt.allocationSize(value.`bytes`) +
            FfiConverterOptionalSequenceTypePrecomposedInput.allocationSize(value.`inputs`) +
            FfiConverterOptionalSequenceTypePrecomposedOutput.allocationSize(value.`outputs`) +
            FfiConverterOptionalSequenceUInt.allocationSize(value.`outputsPermutation`)
    )

    override fun write(value: PrecomposedTransaction, buf: ByteBuffer) {
        FfiConverterString.write(value.`txType`, buf)
        FfiConverterOptionalString.write(value.`totalSpent`, buf)
        FfiConverterOptionalString.write(value.`fee`, buf)
        FfiConverterOptionalString.write(value.`feePerByte`, buf)
        FfiConverterOptionalUInt.write(value.`bytes`, buf)
        FfiConverterOptionalSequenceTypePrecomposedInput.write(value.`inputs`, buf)
        FfiConverterOptionalSequenceTypePrecomposedOutput.write(value.`outputs`, buf)
        FfiConverterOptionalSequenceUInt.write(value.`outputsPermutation`, buf)
    }
}




public object FfiConverterTypePubkyAuth: FfiConverterRustBuffer<PubkyAuth> {
    override fun read(buf: ByteBuffer): PubkyAuth {
        return PubkyAuth(
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: PubkyAuth): ULong = (
            FfiConverterString.allocationSize(value.`data`)
    )

    override fun write(value: PubkyAuth, buf: ByteBuffer) {
        FfiConverterString.write(value.`data`, buf)
    }
}




public object FfiConverterTypePublicKeyResponse: FfiConverterRustBuffer<PublicKeyResponse> {
    override fun read(buf: ByteBuffer): PublicKeyResponse {
        return PublicKeyResponse(
            FfiConverterSequenceUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterOptionalString.read(buf),
        )
    }

    override fun allocationSize(value: PublicKeyResponse): ULong = (
            FfiConverterSequenceUInt.allocationSize(value.`path`) +
            FfiConverterString.allocationSize(value.`serializedPath`) +
            FfiConverterString.allocationSize(value.`xpub`) +
            FfiConverterOptionalString.allocationSize(value.`xpubSegwit`) +
            FfiConverterString.allocationSize(value.`chainCode`) +
            FfiConverterUInt.allocationSize(value.`childNum`) +
            FfiConverterString.allocationSize(value.`publicKey`) +
            FfiConverterUInt.allocationSize(value.`fingerprint`) +
            FfiConverterUInt.allocationSize(value.`depth`) +
            FfiConverterOptionalString.allocationSize(value.`descriptor`)
    )

    override fun write(value: PublicKeyResponse, buf: ByteBuffer) {
        FfiConverterSequenceUInt.write(value.`path`, buf)
        FfiConverterString.write(value.`serializedPath`, buf)
        FfiConverterString.write(value.`xpub`, buf)
        FfiConverterOptionalString.write(value.`xpubSegwit`, buf)
        FfiConverterString.write(value.`chainCode`, buf)
        FfiConverterUInt.write(value.`childNum`, buf)
        FfiConverterString.write(value.`publicKey`, buf)
        FfiConverterUInt.write(value.`fingerprint`, buf)
        FfiConverterUInt.write(value.`depth`, buf)
        FfiConverterOptionalString.write(value.`descriptor`, buf)
    }
}




public object FfiConverterTypeRefTransaction: FfiConverterRustBuffer<RefTransaction> {
    override fun read(buf: ByteBuffer): RefTransaction {
        return RefTransaction(
            FfiConverterString.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterSequenceTypeRefTxInput.read(buf),
            FfiConverterSequenceTypeRefTxOutput.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalString.read(buf),
        )
    }

    override fun allocationSize(value: RefTransaction): ULong = (
            FfiConverterString.allocationSize(value.`hash`) +
            FfiConverterOptionalUInt.allocationSize(value.`version`) +
            FfiConverterSequenceTypeRefTxInput.allocationSize(value.`inputs`) +
            FfiConverterSequenceTypeRefTxOutput.allocationSize(value.`binOutputs`) +
            FfiConverterOptionalUInt.allocationSize(value.`lockTime`) +
            FfiConverterOptionalUInt.allocationSize(value.`expiry`) +
            FfiConverterOptionalUInt.allocationSize(value.`versionGroupId`) +
            FfiConverterOptionalBoolean.allocationSize(value.`overwintered`) +
            FfiConverterOptionalUInt.allocationSize(value.`timestamp`) +
            FfiConverterOptionalUInt.allocationSize(value.`branchId`) +
            FfiConverterOptionalString.allocationSize(value.`extraData`)
    )

    override fun write(value: RefTransaction, buf: ByteBuffer) {
        FfiConverterString.write(value.`hash`, buf)
        FfiConverterOptionalUInt.write(value.`version`, buf)
        FfiConverterSequenceTypeRefTxInput.write(value.`inputs`, buf)
        FfiConverterSequenceTypeRefTxOutput.write(value.`binOutputs`, buf)
        FfiConverterOptionalUInt.write(value.`lockTime`, buf)
        FfiConverterOptionalUInt.write(value.`expiry`, buf)
        FfiConverterOptionalUInt.write(value.`versionGroupId`, buf)
        FfiConverterOptionalBoolean.write(value.`overwintered`, buf)
        FfiConverterOptionalUInt.write(value.`timestamp`, buf)
        FfiConverterOptionalUInt.write(value.`branchId`, buf)
        FfiConverterOptionalString.write(value.`extraData`, buf)
    }
}




public object FfiConverterTypeRefTxInput: FfiConverterRustBuffer<RefTxInput> {
    override fun read(buf: ByteBuffer): RefTxInput {
        return RefTxInput(
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
        )
    }

    override fun allocationSize(value: RefTxInput): ULong = (
            FfiConverterString.allocationSize(value.`prevHash`) +
            FfiConverterUInt.allocationSize(value.`prevIndex`) +
            FfiConverterString.allocationSize(value.`scriptSig`) +
            FfiConverterUInt.allocationSize(value.`sequence`)
    )

    override fun write(value: RefTxInput, buf: ByteBuffer) {
        FfiConverterString.write(value.`prevHash`, buf)
        FfiConverterUInt.write(value.`prevIndex`, buf)
        FfiConverterString.write(value.`scriptSig`, buf)
        FfiConverterUInt.write(value.`sequence`, buf)
    }
}




public object FfiConverterTypeRefTxOutput: FfiConverterRustBuffer<RefTxOutput> {
    override fun read(buf: ByteBuffer): RefTxOutput {
        return RefTxOutput(
            FfiConverterULong.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: RefTxOutput): ULong = (
            FfiConverterULong.allocationSize(value.`amount`) +
            FfiConverterString.allocationSize(value.`scriptPubkey`)
    )

    override fun write(value: RefTxOutput, buf: ByteBuffer) {
        FfiConverterULong.write(value.`amount`, buf)
        FfiConverterString.write(value.`scriptPubkey`, buf)
    }
}




public object FfiConverterTypeRefundMemo: FfiConverterRustBuffer<RefundMemo> {
    override fun read(buf: ByteBuffer): RefundMemo {
        return RefundMemo(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: RefundMemo): ULong = (
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterString.allocationSize(value.`mac`)
    )

    override fun write(value: RefundMemo, buf: ByteBuffer) {
        FfiConverterString.write(value.`address`, buf)
        FfiConverterString.write(value.`mac`, buf)
    }
}




public object FfiConverterTypeSignedTransactionResponse: FfiConverterRustBuffer<SignedTransactionResponse> {
    override fun read(buf: ByteBuffer): SignedTransactionResponse {
        return SignedTransactionResponse(
            FfiConverterSequenceString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
        )
    }

    override fun allocationSize(value: SignedTransactionResponse): ULong = (
            FfiConverterSequenceString.allocationSize(value.`signatures`) +
            FfiConverterString.allocationSize(value.`serializedTx`) +
            FfiConverterOptionalString.allocationSize(value.`txid`)
    )

    override fun write(value: SignedTransactionResponse, buf: ByteBuffer) {
        FfiConverterSequenceString.write(value.`signatures`, buf)
        FfiConverterString.write(value.`serializedTx`, buf)
        FfiConverterOptionalString.write(value.`txid`, buf)
    }
}




public object FfiConverterTypeTextMemo: FfiConverterRustBuffer<TextMemo> {
    override fun read(buf: ByteBuffer): TextMemo {
        return TextMemo(
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: TextMemo): ULong = (
            FfiConverterString.allocationSize(value.`text`)
    )

    override fun write(value: TextMemo, buf: ByteBuffer) {
        FfiConverterString.write(value.`text`, buf)
    }
}




public object FfiConverterTypeTxAckPaymentRequest: FfiConverterRustBuffer<TxAckPaymentRequest> {
    override fun read(buf: ByteBuffer): TxAckPaymentRequest {
        return TxAckPaymentRequest(
            FfiConverterOptionalString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalSequenceTypePaymentRequestMemo.read(buf),
            FfiConverterOptionalULong.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: TxAckPaymentRequest): ULong = (
            FfiConverterOptionalString.allocationSize(value.`nonce`) +
            FfiConverterString.allocationSize(value.`recipientName`) +
            FfiConverterOptionalSequenceTypePaymentRequestMemo.allocationSize(value.`memos`) +
            FfiConverterOptionalULong.allocationSize(value.`amount`) +
            FfiConverterString.allocationSize(value.`signature`)
    )

    override fun write(value: TxAckPaymentRequest, buf: ByteBuffer) {
        FfiConverterOptionalString.write(value.`nonce`, buf)
        FfiConverterString.write(value.`recipientName`, buf)
        FfiConverterOptionalSequenceTypePaymentRequestMemo.write(value.`memos`, buf)
        FfiConverterOptionalULong.write(value.`amount`, buf)
        FfiConverterString.write(value.`signature`, buf)
    }
}




public object FfiConverterTypeTxInputType: FfiConverterRustBuffer<TxInputType> {
    override fun read(buf: ByteBuffer): TxInputType {
        return TxInputType(
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalSequenceUInt.read(buf),
            FfiConverterOptionalTypeScriptType.read(buf),
            FfiConverterOptionalTypeMultisigRedeemScriptType.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalUInt.read(buf),
        )
    }

    override fun allocationSize(value: TxInputType): ULong = (
            FfiConverterString.allocationSize(value.`prevHash`) +
            FfiConverterUInt.allocationSize(value.`prevIndex`) +
            FfiConverterULong.allocationSize(value.`amount`) +
            FfiConverterOptionalUInt.allocationSize(value.`sequence`) +
            FfiConverterOptionalSequenceUInt.allocationSize(value.`addressN`) +
            FfiConverterOptionalTypeScriptType.allocationSize(value.`scriptType`) +
            FfiConverterOptionalTypeMultisigRedeemScriptType.allocationSize(value.`multisig`) +
            FfiConverterOptionalString.allocationSize(value.`scriptPubkey`) +
            FfiConverterOptionalString.allocationSize(value.`scriptSig`) +
            FfiConverterOptionalString.allocationSize(value.`witness`) +
            FfiConverterOptionalString.allocationSize(value.`ownershipProof`) +
            FfiConverterOptionalString.allocationSize(value.`commitmentData`) +
            FfiConverterOptionalString.allocationSize(value.`origHash`) +
            FfiConverterOptionalUInt.allocationSize(value.`origIndex`) +
            FfiConverterOptionalUInt.allocationSize(value.`coinjoinFlags`)
    )

    override fun write(value: TxInputType, buf: ByteBuffer) {
        FfiConverterString.write(value.`prevHash`, buf)
        FfiConverterUInt.write(value.`prevIndex`, buf)
        FfiConverterULong.write(value.`amount`, buf)
        FfiConverterOptionalUInt.write(value.`sequence`, buf)
        FfiConverterOptionalSequenceUInt.write(value.`addressN`, buf)
        FfiConverterOptionalTypeScriptType.write(value.`scriptType`, buf)
        FfiConverterOptionalTypeMultisigRedeemScriptType.write(value.`multisig`, buf)
        FfiConverterOptionalString.write(value.`scriptPubkey`, buf)
        FfiConverterOptionalString.write(value.`scriptSig`, buf)
        FfiConverterOptionalString.write(value.`witness`, buf)
        FfiConverterOptionalString.write(value.`ownershipProof`, buf)
        FfiConverterOptionalString.write(value.`commitmentData`, buf)
        FfiConverterOptionalString.write(value.`origHash`, buf)
        FfiConverterOptionalUInt.write(value.`origIndex`, buf)
        FfiConverterOptionalUInt.write(value.`coinjoinFlags`, buf)
    }
}




public object FfiConverterTypeTxOutputType: FfiConverterRustBuffer<TxOutputType> {
    override fun read(buf: ByteBuffer): TxOutputType {
        return TxOutputType(
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalSequenceUInt.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterTypeScriptType.read(buf),
            FfiConverterOptionalTypeMultisigRedeemScriptType.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalUInt.read(buf),
        )
    }

    override fun allocationSize(value: TxOutputType): ULong = (
            FfiConverterOptionalString.allocationSize(value.`address`) +
            FfiConverterOptionalSequenceUInt.allocationSize(value.`addressN`) +
            FfiConverterULong.allocationSize(value.`amount`) +
            FfiConverterTypeScriptType.allocationSize(value.`scriptType`) +
            FfiConverterOptionalTypeMultisigRedeemScriptType.allocationSize(value.`multisig`) +
            FfiConverterOptionalString.allocationSize(value.`opReturnData`) +
            FfiConverterOptionalString.allocationSize(value.`origHash`) +
            FfiConverterOptionalUInt.allocationSize(value.`origIndex`) +
            FfiConverterOptionalUInt.allocationSize(value.`paymentReqIndex`)
    )

    override fun write(value: TxOutputType, buf: ByteBuffer) {
        FfiConverterOptionalString.write(value.`address`, buf)
        FfiConverterOptionalSequenceUInt.write(value.`addressN`, buf)
        FfiConverterULong.write(value.`amount`, buf)
        FfiConverterTypeScriptType.write(value.`scriptType`, buf)
        FfiConverterOptionalTypeMultisigRedeemScriptType.write(value.`multisig`, buf)
        FfiConverterOptionalString.write(value.`opReturnData`, buf)
        FfiConverterOptionalString.write(value.`origHash`, buf)
        FfiConverterOptionalUInt.write(value.`origIndex`, buf)
        FfiConverterOptionalUInt.write(value.`paymentReqIndex`, buf)
    }
}




public object FfiConverterTypeUnlockPath: FfiConverterRustBuffer<UnlockPath> {
    override fun read(buf: ByteBuffer): UnlockPath {
        return UnlockPath(
            FfiConverterSequenceUInt.read(buf),
            FfiConverterOptionalString.read(buf),
        )
    }

    override fun allocationSize(value: UnlockPath): ULong = (
            FfiConverterSequenceUInt.allocationSize(value.`addressN`) +
            FfiConverterOptionalString.allocationSize(value.`mac`)
    )

    override fun write(value: UnlockPath, buf: ByteBuffer) {
        FfiConverterSequenceUInt.write(value.`addressN`, buf)
        FfiConverterOptionalString.write(value.`mac`, buf)
    }
}




public object FfiConverterTypeValidationResult: FfiConverterRustBuffer<ValidationResult> {
    override fun read(buf: ByteBuffer): ValidationResult {
        return ValidationResult(
            FfiConverterString.read(buf),
            FfiConverterTypeNetworkType.read(buf),
            FfiConverterTypeAddressType.read(buf),
        )
    }

    override fun allocationSize(value: ValidationResult): ULong = (
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterTypeNetworkType.allocationSize(value.`network`) +
            FfiConverterTypeAddressType.allocationSize(value.`addressType`)
    )

    override fun write(value: ValidationResult, buf: ByteBuffer) {
        FfiConverterString.write(value.`address`, buf)
        FfiConverterTypeNetworkType.write(value.`network`, buf)
        FfiConverterTypeAddressType.write(value.`addressType`, buf)
    }
}




public object FfiConverterTypeVerifyMessageResponse: FfiConverterRustBuffer<VerifyMessageResponse> {
    override fun read(buf: ByteBuffer): VerifyMessageResponse {
        return VerifyMessageResponse(
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: VerifyMessageResponse): ULong = (
            FfiConverterString.allocationSize(value.`message`)
    )

    override fun write(value: VerifyMessageResponse, buf: ByteBuffer) {
        FfiConverterString.write(value.`message`, buf)
    }
}




public object FfiConverterTypeXrpMarker: FfiConverterRustBuffer<XrpMarker> {
    override fun read(buf: ByteBuffer): XrpMarker {
        return XrpMarker(
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
        )
    }

    override fun allocationSize(value: XrpMarker): ULong = (
            FfiConverterULong.allocationSize(value.`ledger`) +
            FfiConverterULong.allocationSize(value.`seq`)
    )

    override fun write(value: XrpMarker, buf: ByteBuffer) {
        FfiConverterULong.write(value.`ledger`, buf)
        FfiConverterULong.write(value.`seq`, buf)
    }
}





public object FfiConverterTypeAccountInfoDetails: FfiConverterRustBuffer<AccountInfoDetails> {
    override fun read(buf: ByteBuffer): AccountInfoDetails = try {
        AccountInfoDetails.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: AccountInfoDetails): ULong = 4UL

    override fun write(value: AccountInfoDetails, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeActivity : FfiConverterRustBuffer<Activity>{
    override fun read(buf: ByteBuffer): Activity {
        return when(buf.getInt()) {
            1 -> Activity.Onchain(
                FfiConverterTypeOnchainActivity.read(buf),
                )
            2 -> Activity.Lightning(
                FfiConverterTypeLightningActivity.read(buf),
                )
            else -> throw RuntimeException("invalid enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: Activity): ULong = when(value) {
        is Activity.Onchain -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeOnchainActivity.allocationSize(value.v1)
            )
        }
        is Activity.Lightning -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeLightningActivity.allocationSize(value.v1)
            )
        }
    }

    override fun write(value: Activity, buf: ByteBuffer) {
        when(value) {
            is Activity.Onchain -> {
                buf.putInt(1)
                FfiConverterTypeOnchainActivity.write(value.v1, buf)
                Unit
            }
            is Activity.Lightning -> {
                buf.putInt(2)
                FfiConverterTypeLightningActivity.write(value.v1, buf)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}




public object ActivityExceptionErrorHandler : UniffiRustCallStatusErrorHandler<ActivityException> {
    override fun lift(errorBuf: RustBufferByValue): ActivityException = FfiConverterTypeActivityError.lift(errorBuf)
}

public object FfiConverterTypeActivityError : FfiConverterRustBuffer<ActivityException> {
    override fun read(buf: ByteBuffer): ActivityException {
        return when (buf.getInt()) {
            1 -> ActivityException.InvalidActivity(
                FfiConverterString.read(buf),
                )
            2 -> ActivityException.InitializationException(
                FfiConverterString.read(buf),
                )
            3 -> ActivityException.InsertException(
                FfiConverterString.read(buf),
                )
            4 -> ActivityException.RetrievalException(
                FfiConverterString.read(buf),
                )
            5 -> ActivityException.DataException(
                FfiConverterString.read(buf),
                )
            6 -> ActivityException.ConnectionException(
                FfiConverterString.read(buf),
                )
            7 -> ActivityException.SerializationException(
                FfiConverterString.read(buf),
                )
            else -> throw RuntimeException("invalid error enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: ActivityException): ULong {
        return when (value) {
            is ActivityException.InvalidActivity -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is ActivityException.InitializationException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is ActivityException.InsertException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is ActivityException.RetrievalException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is ActivityException.DataException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is ActivityException.ConnectionException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is ActivityException.SerializationException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
        }
    }

    override fun write(value: ActivityException, buf: ByteBuffer) {
        when (value) {
            is ActivityException.InvalidActivity -> {
                buf.putInt(1)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is ActivityException.InitializationException -> {
                buf.putInt(2)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is ActivityException.InsertException -> {
                buf.putInt(3)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is ActivityException.RetrievalException -> {
                buf.putInt(4)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is ActivityException.DataException -> {
                buf.putInt(5)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is ActivityException.ConnectionException -> {
                buf.putInt(6)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is ActivityException.SerializationException -> {
                buf.putInt(7)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}





public object FfiConverterTypeActivityFilter: FfiConverterRustBuffer<ActivityFilter> {
    override fun read(buf: ByteBuffer): ActivityFilter = try {
        ActivityFilter.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: ActivityFilter): ULong = 4UL

    override fun write(value: ActivityFilter, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeActivityType: FfiConverterRustBuffer<ActivityType> {
    override fun read(buf: ByteBuffer): ActivityType = try {
        ActivityType.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: ActivityType): ULong = 4UL

    override fun write(value: ActivityType, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}




public object AddressExceptionErrorHandler : UniffiRustCallStatusErrorHandler<AddressException> {
    override fun lift(errorBuf: RustBufferByValue): AddressException = FfiConverterTypeAddressError.lift(errorBuf)
}

public object FfiConverterTypeAddressError : FfiConverterRustBuffer<AddressException> {
    override fun read(buf: ByteBuffer): AddressException {
        return when (buf.getInt()) {
            1 -> AddressException.InvalidAddress()
            2 -> AddressException.InvalidNetwork()
            3 -> AddressException.MnemonicGenerationFailed()
            4 -> AddressException.InvalidMnemonic()
            5 -> AddressException.InvalidEntropy()
            6 -> AddressException.AddressDerivationFailed()
            else -> throw RuntimeException("invalid error enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: AddressException): ULong {
        return when (value) {
            is AddressException.InvalidAddress -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is AddressException.InvalidNetwork -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is AddressException.MnemonicGenerationFailed -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is AddressException.InvalidMnemonic -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is AddressException.InvalidEntropy -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is AddressException.AddressDerivationFailed -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
        }
    }

    override fun write(value: AddressException, buf: ByteBuffer) {
        when (value) {
            is AddressException.InvalidAddress -> {
                buf.putInt(1)
                Unit
            }
            is AddressException.InvalidNetwork -> {
                buf.putInt(2)
                Unit
            }
            is AddressException.MnemonicGenerationFailed -> {
                buf.putInt(3)
                Unit
            }
            is AddressException.InvalidMnemonic -> {
                buf.putInt(4)
                Unit
            }
            is AddressException.InvalidEntropy -> {
                buf.putInt(5)
                Unit
            }
            is AddressException.AddressDerivationFailed -> {
                buf.putInt(6)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}





public object FfiConverterTypeAddressType: FfiConverterRustBuffer<AddressType> {
    override fun read(buf: ByteBuffer): AddressType = try {
        AddressType.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: AddressType): ULong = 4UL

    override fun write(value: AddressType, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeAmountUnit: FfiConverterRustBuffer<AmountUnit> {
    override fun read(buf: ByteBuffer): AmountUnit = try {
        AmountUnit.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: AmountUnit): ULong = 4UL

    override fun write(value: AmountUnit, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeBitcoinNetworkEnum: FfiConverterRustBuffer<BitcoinNetworkEnum> {
    override fun read(buf: ByteBuffer): BitcoinNetworkEnum = try {
        BitcoinNetworkEnum.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: BitcoinNetworkEnum): ULong = 4UL

    override fun write(value: BitcoinNetworkEnum, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}




public object BlocktankExceptionErrorHandler : UniffiRustCallStatusErrorHandler<BlocktankException> {
    override fun lift(errorBuf: RustBufferByValue): BlocktankException = FfiConverterTypeBlocktankError.lift(errorBuf)
}

public object FfiConverterTypeBlocktankError : FfiConverterRustBuffer<BlocktankException> {
    override fun read(buf: ByteBuffer): BlocktankException {
        return when (buf.getInt()) {
            1 -> BlocktankException.HttpClient(
                FfiConverterString.read(buf),
                )
            2 -> BlocktankException.BlocktankClient(
                FfiConverterString.read(buf),
                )
            3 -> BlocktankException.InvalidBlocktank(
                FfiConverterString.read(buf),
                )
            4 -> BlocktankException.InitializationException(
                FfiConverterString.read(buf),
                )
            5 -> BlocktankException.InsertException(
                FfiConverterString.read(buf),
                )
            6 -> BlocktankException.RetrievalException(
                FfiConverterString.read(buf),
                )
            7 -> BlocktankException.DataException(
                FfiConverterString.read(buf),
                )
            8 -> BlocktankException.ConnectionException(
                FfiConverterString.read(buf),
                )
            9 -> BlocktankException.SerializationException(
                FfiConverterString.read(buf),
                )
            10 -> BlocktankException.ChannelOpen(
                FfiConverterTypeBtChannelOrderErrorType.read(buf),
                FfiConverterString.read(buf),
                )
            11 -> BlocktankException.OrderState(
                FfiConverterString.read(buf),
                )
            12 -> BlocktankException.InvalidParameter(
                FfiConverterString.read(buf),
                )
            13 -> BlocktankException.DatabaseException(
                FfiConverterString.read(buf),
                )
            else -> throw RuntimeException("invalid error enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: BlocktankException): ULong {
        return when (value) {
            is BlocktankException.HttpClient -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is BlocktankException.BlocktankClient -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is BlocktankException.InvalidBlocktank -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is BlocktankException.InitializationException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is BlocktankException.InsertException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is BlocktankException.RetrievalException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is BlocktankException.DataException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is BlocktankException.ConnectionException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is BlocktankException.SerializationException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is BlocktankException.ChannelOpen -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterTypeBtChannelOrderErrorType.allocationSize(value.`errorType`)
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is BlocktankException.OrderState -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is BlocktankException.InvalidParameter -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is BlocktankException.DatabaseException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
        }
    }

    override fun write(value: BlocktankException, buf: ByteBuffer) {
        when (value) {
            is BlocktankException.HttpClient -> {
                buf.putInt(1)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is BlocktankException.BlocktankClient -> {
                buf.putInt(2)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is BlocktankException.InvalidBlocktank -> {
                buf.putInt(3)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is BlocktankException.InitializationException -> {
                buf.putInt(4)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is BlocktankException.InsertException -> {
                buf.putInt(5)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is BlocktankException.RetrievalException -> {
                buf.putInt(6)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is BlocktankException.DataException -> {
                buf.putInt(7)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is BlocktankException.ConnectionException -> {
                buf.putInt(8)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is BlocktankException.SerializationException -> {
                buf.putInt(9)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is BlocktankException.ChannelOpen -> {
                buf.putInt(10)
                FfiConverterTypeBtChannelOrderErrorType.write(value.`errorType`, buf)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is BlocktankException.OrderState -> {
                buf.putInt(11)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is BlocktankException.InvalidParameter -> {
                buf.putInt(12)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is BlocktankException.DatabaseException -> {
                buf.putInt(13)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}





public object FfiConverterTypeBtBolt11InvoiceState: FfiConverterRustBuffer<BtBolt11InvoiceState> {
    override fun read(buf: ByteBuffer): BtBolt11InvoiceState = try {
        BtBolt11InvoiceState.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: BtBolt11InvoiceState): ULong = 4UL

    override fun write(value: BtBolt11InvoiceState, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeBtChannelOrderErrorType: FfiConverterRustBuffer<BtChannelOrderErrorType> {
    override fun read(buf: ByteBuffer): BtChannelOrderErrorType = try {
        BtChannelOrderErrorType.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: BtChannelOrderErrorType): ULong = 4UL

    override fun write(value: BtChannelOrderErrorType, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeBtOpenChannelState: FfiConverterRustBuffer<BtOpenChannelState> {
    override fun read(buf: ByteBuffer): BtOpenChannelState = try {
        BtOpenChannelState.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: BtOpenChannelState): ULong = 4UL

    override fun write(value: BtOpenChannelState, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeBtOrderState: FfiConverterRustBuffer<BtOrderState> {
    override fun read(buf: ByteBuffer): BtOrderState = try {
        BtOrderState.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: BtOrderState): ULong = 4UL

    override fun write(value: BtOrderState, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeBtOrderState2: FfiConverterRustBuffer<BtOrderState2> {
    override fun read(buf: ByteBuffer): BtOrderState2 = try {
        BtOrderState2.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: BtOrderState2): ULong = 4UL

    override fun write(value: BtOrderState2, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeBtPaymentState: FfiConverterRustBuffer<BtPaymentState> {
    override fun read(buf: ByteBuffer): BtPaymentState = try {
        BtPaymentState.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: BtPaymentState): ULong = 4UL

    override fun write(value: BtPaymentState, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeBtPaymentState2: FfiConverterRustBuffer<BtPaymentState2> {
    override fun read(buf: ByteBuffer): BtPaymentState2 = try {
        BtPaymentState2.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: BtPaymentState2): ULong = 4UL

    override fun write(value: BtPaymentState2, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeCJitStateEnum: FfiConverterRustBuffer<CJitStateEnum> {
    override fun read(buf: ByteBuffer): CJitStateEnum = try {
        CJitStateEnum.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: CJitStateEnum): ULong = 4UL

    override fun write(value: CJitStateEnum, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeComposeOutput : FfiConverterRustBuffer<ComposeOutput>{
    override fun read(buf: ByteBuffer): ComposeOutput {
        return when(buf.getInt()) {
            1 -> ComposeOutput.Regular(
                FfiConverterString.read(buf),
                FfiConverterString.read(buf),
                )
            2 -> ComposeOutput.SendMax(
                FfiConverterString.read(buf),
                )
            3 -> ComposeOutput.OpReturn(
                FfiConverterString.read(buf),
                )
            4 -> ComposeOutput.PaymentNoAddress(
                FfiConverterString.read(buf),
                )
            5 -> ComposeOutput.SendMaxNoAddress
            else -> throw RuntimeException("invalid enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: ComposeOutput): ULong = when(value) {
        is ComposeOutput.Regular -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterString.allocationSize(value.`amount`)
                + FfiConverterString.allocationSize(value.`address`)
            )
        }
        is ComposeOutput.SendMax -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterString.allocationSize(value.`address`)
            )
        }
        is ComposeOutput.OpReturn -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterString.allocationSize(value.`dataHex`)
            )
        }
        is ComposeOutput.PaymentNoAddress -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterString.allocationSize(value.`amount`)
            )
        }
        is ComposeOutput.SendMaxNoAddress -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
            )
        }
    }

    override fun write(value: ComposeOutput, buf: ByteBuffer) {
        when(value) {
            is ComposeOutput.Regular -> {
                buf.putInt(1)
                FfiConverterString.write(value.`amount`, buf)
                FfiConverterString.write(value.`address`, buf)
                Unit
            }
            is ComposeOutput.SendMax -> {
                buf.putInt(2)
                FfiConverterString.write(value.`address`, buf)
                Unit
            }
            is ComposeOutput.OpReturn -> {
                buf.putInt(3)
                FfiConverterString.write(value.`dataHex`, buf)
                Unit
            }
            is ComposeOutput.PaymentNoAddress -> {
                buf.putInt(4)
                FfiConverterString.write(value.`amount`, buf)
                Unit
            }
            is ComposeOutput.SendMaxNoAddress -> {
                buf.putInt(5)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}





public object FfiConverterTypeComposeTransactionResponse : FfiConverterRustBuffer<ComposeTransactionResponse>{
    override fun read(buf: ByteBuffer): ComposeTransactionResponse {
        return when(buf.getInt()) {
            1 -> ComposeTransactionResponse.SignedTransaction(
                FfiConverterTypeSignedTransactionResponse.read(buf),
                )
            2 -> ComposeTransactionResponse.PrecomposedTransactions(
                FfiConverterSequenceTypePrecomposedTransaction.read(buf),
                )
            else -> throw RuntimeException("invalid enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: ComposeTransactionResponse): ULong = when(value) {
        is ComposeTransactionResponse.SignedTransaction -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeSignedTransactionResponse.allocationSize(value.v1)
            )
        }
        is ComposeTransactionResponse.PrecomposedTransactions -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterSequenceTypePrecomposedTransaction.allocationSize(value.v1)
            )
        }
    }

    override fun write(value: ComposeTransactionResponse, buf: ByteBuffer) {
        when(value) {
            is ComposeTransactionResponse.SignedTransaction -> {
                buf.putInt(1)
                FfiConverterTypeSignedTransactionResponse.write(value.v1, buf)
                Unit
            }
            is ComposeTransactionResponse.PrecomposedTransactions -> {
                buf.putInt(2)
                FfiConverterSequenceTypePrecomposedTransaction.write(value.v1, buf)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}




public object DbExceptionErrorHandler : UniffiRustCallStatusErrorHandler<DbException> {
    override fun lift(errorBuf: RustBufferByValue): DbException = FfiConverterTypeDbError.lift(errorBuf)
}

public object FfiConverterTypeDbError : FfiConverterRustBuffer<DbException> {
    override fun read(buf: ByteBuffer): DbException {
        return when (buf.getInt()) {
            1 -> DbException.DbActivityException(
                FfiConverterTypeActivityError.read(buf),
                )
            2 -> DbException.DbBlocktankException(
                FfiConverterTypeBlocktankError.read(buf),
                )
            3 -> DbException.InitializationException(
                FfiConverterString.read(buf),
                )
            else -> throw RuntimeException("invalid error enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: DbException): ULong {
        return when (value) {
            is DbException.DbActivityException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterTypeActivityError.allocationSize(value.`errorDetails`)
            )
            is DbException.DbBlocktankException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterTypeBlocktankError.allocationSize(value.`errorDetails`)
            )
            is DbException.InitializationException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
        }
    }

    override fun write(value: DbException, buf: ByteBuffer) {
        when (value) {
            is DbException.DbActivityException -> {
                buf.putInt(1)
                FfiConverterTypeActivityError.write(value.`errorDetails`, buf)
                Unit
            }
            is DbException.DbBlocktankException -> {
                buf.putInt(2)
                FfiConverterTypeBlocktankError.write(value.`errorDetails`, buf)
                Unit
            }
            is DbException.InitializationException -> {
                buf.putInt(3)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}




public object DecodingExceptionErrorHandler : UniffiRustCallStatusErrorHandler<DecodingException> {
    override fun lift(errorBuf: RustBufferByValue): DecodingException = FfiConverterTypeDecodingError.lift(errorBuf)
}

public object FfiConverterTypeDecodingError : FfiConverterRustBuffer<DecodingException> {
    override fun read(buf: ByteBuffer): DecodingException {
        return when (buf.getInt()) {
            1 -> DecodingException.InvalidFormat()
            2 -> DecodingException.InvalidNetwork()
            3 -> DecodingException.InvalidAmount()
            4 -> DecodingException.InvalidLnurlPayAmount(
                FfiConverterULong.read(buf),
                FfiConverterULong.read(buf),
                FfiConverterULong.read(buf),
                )
            5 -> DecodingException.InvalidTimestamp()
            6 -> DecodingException.InvalidChecksum()
            7 -> DecodingException.InvalidResponse()
            8 -> DecodingException.UnsupportedType()
            9 -> DecodingException.InvalidAddress()
            10 -> DecodingException.RequestFailed()
            11 -> DecodingException.ClientCreationFailed()
            12 -> DecodingException.InvoiceCreationFailed(
                FfiConverterString.read(buf),
                )
            else -> throw RuntimeException("invalid error enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: DecodingException): ULong {
        return when (value) {
            is DecodingException.InvalidFormat -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is DecodingException.InvalidNetwork -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is DecodingException.InvalidAmount -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is DecodingException.InvalidLnurlPayAmount -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterULong.allocationSize(value.`amountSatoshis`)
                + FfiConverterULong.allocationSize(value.`min`)
                + FfiConverterULong.allocationSize(value.`max`)
            )
            is DecodingException.InvalidTimestamp -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is DecodingException.InvalidChecksum -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is DecodingException.InvalidResponse -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is DecodingException.UnsupportedType -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is DecodingException.InvalidAddress -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is DecodingException.RequestFailed -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is DecodingException.ClientCreationFailed -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is DecodingException.InvoiceCreationFailed -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorMessage`)
            )
        }
    }

    override fun write(value: DecodingException, buf: ByteBuffer) {
        when (value) {
            is DecodingException.InvalidFormat -> {
                buf.putInt(1)
                Unit
            }
            is DecodingException.InvalidNetwork -> {
                buf.putInt(2)
                Unit
            }
            is DecodingException.InvalidAmount -> {
                buf.putInt(3)
                Unit
            }
            is DecodingException.InvalidLnurlPayAmount -> {
                buf.putInt(4)
                FfiConverterULong.write(value.`amountSatoshis`, buf)
                FfiConverterULong.write(value.`min`, buf)
                FfiConverterULong.write(value.`max`, buf)
                Unit
            }
            is DecodingException.InvalidTimestamp -> {
                buf.putInt(5)
                Unit
            }
            is DecodingException.InvalidChecksum -> {
                buf.putInt(6)
                Unit
            }
            is DecodingException.InvalidResponse -> {
                buf.putInt(7)
                Unit
            }
            is DecodingException.UnsupportedType -> {
                buf.putInt(8)
                Unit
            }
            is DecodingException.InvalidAddress -> {
                buf.putInt(9)
                Unit
            }
            is DecodingException.RequestFailed -> {
                buf.putInt(10)
                Unit
            }
            is DecodingException.ClientCreationFailed -> {
                buf.putInt(11)
                Unit
            }
            is DecodingException.InvoiceCreationFailed -> {
                buf.putInt(12)
                FfiConverterString.write(value.`errorMessage`, buf)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}





public object FfiConverterTypeDefaultAccountType: FfiConverterRustBuffer<DefaultAccountType> {
    override fun read(buf: ByteBuffer): DefaultAccountType = try {
        DefaultAccountType.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: DefaultAccountType): ULong = 4UL

    override fun write(value: DefaultAccountType, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeHDNodeTypeOrString : FfiConverterRustBuffer<HdNodeTypeOrString>{
    override fun read(buf: ByteBuffer): HdNodeTypeOrString {
        return when(buf.getInt()) {
            1 -> HdNodeTypeOrString.String(
                FfiConverterString.read(buf),
                )
            2 -> HdNodeTypeOrString.Node(
                FfiConverterTypeHDNodeType.read(buf),
                )
            else -> throw RuntimeException("invalid enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: HdNodeTypeOrString): ULong = when(value) {
        is HdNodeTypeOrString.String -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterString.allocationSize(value.v1)
            )
        }
        is HdNodeTypeOrString.Node -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeHDNodeType.allocationSize(value.v1)
            )
        }
    }

    override fun write(value: HdNodeTypeOrString, buf: ByteBuffer) {
        when(value) {
            is HdNodeTypeOrString.String -> {
                buf.putInt(1)
                FfiConverterString.write(value.v1, buf)
                Unit
            }
            is HdNodeTypeOrString.Node -> {
                buf.putInt(2)
                FfiConverterTypeHDNodeType.write(value.v1, buf)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}




public object LnurlExceptionErrorHandler : UniffiRustCallStatusErrorHandler<LnurlException> {
    override fun lift(errorBuf: RustBufferByValue): LnurlException = FfiConverterTypeLnurlError.lift(errorBuf)
}

public object FfiConverterTypeLnurlError : FfiConverterRustBuffer<LnurlException> {
    override fun read(buf: ByteBuffer): LnurlException {
        return when (buf.getInt()) {
            1 -> LnurlException.InvalidAddress()
            2 -> LnurlException.ClientCreationFailed()
            3 -> LnurlException.RequestFailed()
            4 -> LnurlException.InvalidResponse()
            5 -> LnurlException.InvalidAmount(
                FfiConverterULong.read(buf),
                FfiConverterULong.read(buf),
                FfiConverterULong.read(buf),
                )
            6 -> LnurlException.InvoiceCreationFailed(
                FfiConverterString.read(buf),
                )
            7 -> LnurlException.AuthenticationFailed()
            else -> throw RuntimeException("invalid error enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: LnurlException): ULong {
        return when (value) {
            is LnurlException.InvalidAddress -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is LnurlException.ClientCreationFailed -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is LnurlException.RequestFailed -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is LnurlException.InvalidResponse -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is LnurlException.InvalidAmount -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterULong.allocationSize(value.`amountSatoshis`)
                + FfiConverterULong.allocationSize(value.`min`)
                + FfiConverterULong.allocationSize(value.`max`)
            )
            is LnurlException.InvoiceCreationFailed -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is LnurlException.AuthenticationFailed -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
        }
    }

    override fun write(value: LnurlException, buf: ByteBuffer) {
        when (value) {
            is LnurlException.InvalidAddress -> {
                buf.putInt(1)
                Unit
            }
            is LnurlException.ClientCreationFailed -> {
                buf.putInt(2)
                Unit
            }
            is LnurlException.RequestFailed -> {
                buf.putInt(3)
                Unit
            }
            is LnurlException.InvalidResponse -> {
                buf.putInt(4)
                Unit
            }
            is LnurlException.InvalidAmount -> {
                buf.putInt(5)
                FfiConverterULong.write(value.`amountSatoshis`, buf)
                FfiConverterULong.write(value.`min`, buf)
                FfiConverterULong.write(value.`max`, buf)
                Unit
            }
            is LnurlException.InvoiceCreationFailed -> {
                buf.putInt(6)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is LnurlException.AuthenticationFailed -> {
                buf.putInt(7)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}





public object FfiConverterTypeManualRefundStateEnum: FfiConverterRustBuffer<ManualRefundStateEnum> {
    override fun read(buf: ByteBuffer): ManualRefundStateEnum = try {
        ManualRefundStateEnum.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: ManualRefundStateEnum): ULong = 4UL

    override fun write(value: ManualRefundStateEnum, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeNetwork: FfiConverterRustBuffer<Network> {
    override fun read(buf: ByteBuffer): Network = try {
        Network.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: Network): ULong = 4UL

    override fun write(value: Network, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeNetworkType: FfiConverterRustBuffer<NetworkType> {
    override fun read(buf: ByteBuffer): NetworkType = try {
        NetworkType.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: NetworkType): ULong = 4UL

    override fun write(value: NetworkType, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypePaymentState: FfiConverterRustBuffer<PaymentState> {
    override fun read(buf: ByteBuffer): PaymentState = try {
        PaymentState.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: PaymentState): ULong = 4UL

    override fun write(value: PaymentState, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypePaymentType: FfiConverterRustBuffer<PaymentType> {
    override fun read(buf: ByteBuffer): PaymentType = try {
        PaymentType.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: PaymentType): ULong = 4UL

    override fun write(value: PaymentType, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeScanner : FfiConverterRustBuffer<Scanner>{
    override fun read(buf: ByteBuffer): Scanner {
        return when(buf.getInt()) {
            1 -> Scanner.OnChain(
                FfiConverterTypeOnChainInvoice.read(buf),
                )
            2 -> Scanner.Lightning(
                FfiConverterTypeLightningInvoice.read(buf),
                )
            3 -> Scanner.PubkyAuth(
                FfiConverterString.read(buf),
                )
            4 -> Scanner.LnurlChannel(
                FfiConverterTypeLnurlChannelData.read(buf),
                )
            5 -> Scanner.LnurlAuth(
                FfiConverterTypeLnurlAuthData.read(buf),
                )
            6 -> Scanner.LnurlWithdraw(
                FfiConverterTypeLnurlWithdrawData.read(buf),
                )
            7 -> Scanner.LnurlAddress(
                FfiConverterTypeLnurlAddressData.read(buf),
                )
            8 -> Scanner.LnurlPay(
                FfiConverterTypeLnurlPayData.read(buf),
                )
            9 -> Scanner.NodeId(
                FfiConverterString.read(buf),
                FfiConverterTypeNetworkType.read(buf),
                )
            10 -> Scanner.Gift(
                FfiConverterString.read(buf),
                FfiConverterULong.read(buf),
                )
            else -> throw RuntimeException("invalid enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: Scanner): ULong = when(value) {
        is Scanner.OnChain -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeOnChainInvoice.allocationSize(value.`invoice`)
            )
        }
        is Scanner.Lightning -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeLightningInvoice.allocationSize(value.`invoice`)
            )
        }
        is Scanner.PubkyAuth -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterString.allocationSize(value.`data`)
            )
        }
        is Scanner.LnurlChannel -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeLnurlChannelData.allocationSize(value.`data`)
            )
        }
        is Scanner.LnurlAuth -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeLnurlAuthData.allocationSize(value.`data`)
            )
        }
        is Scanner.LnurlWithdraw -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeLnurlWithdrawData.allocationSize(value.`data`)
            )
        }
        is Scanner.LnurlAddress -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeLnurlAddressData.allocationSize(value.`data`)
            )
        }
        is Scanner.LnurlPay -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeLnurlPayData.allocationSize(value.`data`)
            )
        }
        is Scanner.NodeId -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterString.allocationSize(value.`url`)
                + FfiConverterTypeNetworkType.allocationSize(value.`network`)
            )
        }
        is Scanner.Gift -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterString.allocationSize(value.`code`)
                + FfiConverterULong.allocationSize(value.`amount`)
            )
        }
    }

    override fun write(value: Scanner, buf: ByteBuffer) {
        when(value) {
            is Scanner.OnChain -> {
                buf.putInt(1)
                FfiConverterTypeOnChainInvoice.write(value.`invoice`, buf)
                Unit
            }
            is Scanner.Lightning -> {
                buf.putInt(2)
                FfiConverterTypeLightningInvoice.write(value.`invoice`, buf)
                Unit
            }
            is Scanner.PubkyAuth -> {
                buf.putInt(3)
                FfiConverterString.write(value.`data`, buf)
                Unit
            }
            is Scanner.LnurlChannel -> {
                buf.putInt(4)
                FfiConverterTypeLnurlChannelData.write(value.`data`, buf)
                Unit
            }
            is Scanner.LnurlAuth -> {
                buf.putInt(5)
                FfiConverterTypeLnurlAuthData.write(value.`data`, buf)
                Unit
            }
            is Scanner.LnurlWithdraw -> {
                buf.putInt(6)
                FfiConverterTypeLnurlWithdrawData.write(value.`data`, buf)
                Unit
            }
            is Scanner.LnurlAddress -> {
                buf.putInt(7)
                FfiConverterTypeLnurlAddressData.write(value.`data`, buf)
                Unit
            }
            is Scanner.LnurlPay -> {
                buf.putInt(8)
                FfiConverterTypeLnurlPayData.write(value.`data`, buf)
                Unit
            }
            is Scanner.NodeId -> {
                buf.putInt(9)
                FfiConverterString.write(value.`url`, buf)
                FfiConverterTypeNetworkType.write(value.`network`, buf)
                Unit
            }
            is Scanner.Gift -> {
                buf.putInt(10)
                FfiConverterString.write(value.`code`, buf)
                FfiConverterULong.write(value.`amount`, buf)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}





public object FfiConverterTypeScriptType: FfiConverterRustBuffer<ScriptType> {
    override fun read(buf: ByteBuffer): ScriptType = try {
        ScriptType.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: ScriptType): ULong = 4UL

    override fun write(value: ScriptType, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeSortDirection: FfiConverterRustBuffer<SortDirection> {
    override fun read(buf: ByteBuffer): SortDirection = try {
        SortDirection.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: SortDirection): ULong = 4UL

    override fun write(value: SortDirection, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeTokenFilter: FfiConverterRustBuffer<TokenFilter> {
    override fun read(buf: ByteBuffer): TokenFilter = try {
        TokenFilter.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: TokenFilter): ULong = 4UL

    override fun write(value: TokenFilter, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}




public object TrezorConnectExceptionErrorHandler : UniffiRustCallStatusErrorHandler<TrezorConnectException> {
    override fun lift(errorBuf: RustBufferByValue): TrezorConnectException = FfiConverterTypeTrezorConnectError.lift(errorBuf)
}

public object FfiConverterTypeTrezorConnectError : FfiConverterRustBuffer<TrezorConnectException> {
    override fun read(buf: ByteBuffer): TrezorConnectException {
        return when (buf.getInt()) {
            1 -> TrezorConnectException.SerdeException(
                FfiConverterString.read(buf),
                )
            2 -> TrezorConnectException.UrlException(
                FfiConverterString.read(buf),
                )
            3 -> TrezorConnectException.EnvironmentException(
                FfiConverterString.read(buf),
                )
            4 -> TrezorConnectException.Other(
                FfiConverterString.read(buf),
                )
            5 -> TrezorConnectException.ClientException(
                FfiConverterString.read(buf),
                )
            else -> throw RuntimeException("invalid error enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: TrezorConnectException): ULong {
        return when (value) {
            is TrezorConnectException.SerdeException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is TrezorConnectException.UrlException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is TrezorConnectException.EnvironmentException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is TrezorConnectException.Other -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is TrezorConnectException.ClientException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
        }
    }

    override fun write(value: TrezorConnectException, buf: ByteBuffer) {
        when (value) {
            is TrezorConnectException.SerdeException -> {
                buf.putInt(1)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is TrezorConnectException.UrlException -> {
                buf.putInt(2)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is TrezorConnectException.EnvironmentException -> {
                buf.putInt(3)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is TrezorConnectException.Other -> {
                buf.putInt(4)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is TrezorConnectException.ClientException -> {
                buf.putInt(5)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}





public object FfiConverterTypeTrezorEnvironment: FfiConverterRustBuffer<TrezorEnvironment> {
    override fun read(buf: ByteBuffer): TrezorEnvironment = try {
        TrezorEnvironment.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: TrezorEnvironment): ULong = 4UL

    override fun write(value: TrezorEnvironment, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeTrezorResponsePayload : FfiConverterRustBuffer<TrezorResponsePayload>{
    override fun read(buf: ByteBuffer): TrezorResponsePayload {
        return when(buf.getInt()) {
            1 -> TrezorResponsePayload.Features(
                FfiConverterTypeFeatureResponse.read(buf),
                )
            2 -> TrezorResponsePayload.Address(
                FfiConverterTypeAddressResponse.read(buf),
                )
            3 -> TrezorResponsePayload.PublicKey(
                FfiConverterTypePublicKeyResponse.read(buf),
                )
            4 -> TrezorResponsePayload.AccountInfo(
                FfiConverterTypeAccountInfoResponse.read(buf),
                )
            5 -> TrezorResponsePayload.ComposeTransaction(
                FfiConverterTypeComposeTransactionResponse.read(buf),
                )
            6 -> TrezorResponsePayload.VerifyMessage(
                FfiConverterTypeVerifyMessageResponse.read(buf),
                )
            7 -> TrezorResponsePayload.MessageSignature(
                FfiConverterTypeMessageSignatureResponse.read(buf),
                )
            8 -> TrezorResponsePayload.SignedTransaction(
                FfiConverterTypeSignedTransactionResponse.read(buf),
                )
            else -> throw RuntimeException("invalid enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: TrezorResponsePayload): ULong = when(value) {
        is TrezorResponsePayload.Features -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeFeatureResponse.allocationSize(value.v1)
            )
        }
        is TrezorResponsePayload.Address -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeAddressResponse.allocationSize(value.v1)
            )
        }
        is TrezorResponsePayload.PublicKey -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypePublicKeyResponse.allocationSize(value.v1)
            )
        }
        is TrezorResponsePayload.AccountInfo -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeAccountInfoResponse.allocationSize(value.v1)
            )
        }
        is TrezorResponsePayload.ComposeTransaction -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeComposeTransactionResponse.allocationSize(value.v1)
            )
        }
        is TrezorResponsePayload.VerifyMessage -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeVerifyMessageResponse.allocationSize(value.v1)
            )
        }
        is TrezorResponsePayload.MessageSignature -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeMessageSignatureResponse.allocationSize(value.v1)
            )
        }
        is TrezorResponsePayload.SignedTransaction -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                + FfiConverterTypeSignedTransactionResponse.allocationSize(value.v1)
            )
        }
    }

    override fun write(value: TrezorResponsePayload, buf: ByteBuffer) {
        when(value) {
            is TrezorResponsePayload.Features -> {
                buf.putInt(1)
                FfiConverterTypeFeatureResponse.write(value.v1, buf)
                Unit
            }
            is TrezorResponsePayload.Address -> {
                buf.putInt(2)
                FfiConverterTypeAddressResponse.write(value.v1, buf)
                Unit
            }
            is TrezorResponsePayload.PublicKey -> {
                buf.putInt(3)
                FfiConverterTypePublicKeyResponse.write(value.v1, buf)
                Unit
            }
            is TrezorResponsePayload.AccountInfo -> {
                buf.putInt(4)
                FfiConverterTypeAccountInfoResponse.write(value.v1, buf)
                Unit
            }
            is TrezorResponsePayload.ComposeTransaction -> {
                buf.putInt(5)
                FfiConverterTypeComposeTransactionResponse.write(value.v1, buf)
                Unit
            }
            is TrezorResponsePayload.VerifyMessage -> {
                buf.putInt(6)
                FfiConverterTypeVerifyMessageResponse.write(value.v1, buf)
                Unit
            }
            is TrezorResponsePayload.MessageSignature -> {
                buf.putInt(7)
                FfiConverterTypeMessageSignatureResponse.write(value.v1, buf)
                Unit
            }
            is TrezorResponsePayload.SignedTransaction -> {
                buf.putInt(8)
                FfiConverterTypeSignedTransactionResponse.write(value.v1, buf)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}





public object FfiConverterTypeWordCount: FfiConverterRustBuffer<WordCount> {
    override fun read(buf: ByteBuffer): WordCount = try {
        WordCount.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: WordCount): ULong = 4UL

    override fun write(value: WordCount, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}




public object FfiConverterOptionalUByte: FfiConverterRustBuffer<kotlin.UByte?> {
    override fun read(buf: ByteBuffer): kotlin.UByte? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterUByte.read(buf)
    }

    override fun allocationSize(value: kotlin.UByte?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterUByte.allocationSize(value)
        }
    }

    override fun write(value: kotlin.UByte?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterUByte.write(value, buf)
        }
    }
}




public object FfiConverterOptionalUInt: FfiConverterRustBuffer<kotlin.UInt?> {
    override fun read(buf: ByteBuffer): kotlin.UInt? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterUInt.read(buf)
    }

    override fun allocationSize(value: kotlin.UInt?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterUInt.allocationSize(value)
        }
    }

    override fun write(value: kotlin.UInt?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterUInt.write(value, buf)
        }
    }
}




public object FfiConverterOptionalULong: FfiConverterRustBuffer<kotlin.ULong?> {
    override fun read(buf: ByteBuffer): kotlin.ULong? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterULong.read(buf)
    }

    override fun allocationSize(value: kotlin.ULong?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterULong.allocationSize(value)
        }
    }

    override fun write(value: kotlin.ULong?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterULong.write(value, buf)
        }
    }
}




public object FfiConverterOptionalBoolean: FfiConverterRustBuffer<kotlin.Boolean?> {
    override fun read(buf: ByteBuffer): kotlin.Boolean? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterBoolean.read(buf)
    }

    override fun allocationSize(value: kotlin.Boolean?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterBoolean.allocationSize(value)
        }
    }

    override fun write(value: kotlin.Boolean?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterBoolean.write(value, buf)
        }
    }
}




public object FfiConverterOptionalString: FfiConverterRustBuffer<kotlin.String?> {
    override fun read(buf: ByteBuffer): kotlin.String? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterString.read(buf)
    }

    override fun allocationSize(value: kotlin.String?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterString.allocationSize(value)
        }
    }

    override fun write(value: kotlin.String?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterString.write(value, buf)
        }
    }
}




public object FfiConverterOptionalByteArray: FfiConverterRustBuffer<kotlin.ByteArray?> {
    override fun read(buf: ByteBuffer): kotlin.ByteArray? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterByteArray.read(buf)
    }

    override fun allocationSize(value: kotlin.ByteArray?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterByteArray.allocationSize(value)
        }
    }

    override fun write(value: kotlin.ByteArray?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterByteArray.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeClosedChannelDetails: FfiConverterRustBuffer<ClosedChannelDetails?> {
    override fun read(buf: ByteBuffer): ClosedChannelDetails? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeClosedChannelDetails.read(buf)
    }

    override fun allocationSize(value: ClosedChannelDetails?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeClosedChannelDetails.allocationSize(value)
        }
    }

    override fun write(value: ClosedChannelDetails?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeClosedChannelDetails.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeCoinPurchaseMemo: FfiConverterRustBuffer<CoinPurchaseMemo?> {
    override fun read(buf: ByteBuffer): CoinPurchaseMemo? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeCoinPurchaseMemo.read(buf)
    }

    override fun allocationSize(value: CoinPurchaseMemo?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeCoinPurchaseMemo.allocationSize(value)
        }
    }

    override fun write(value: CoinPurchaseMemo?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeCoinPurchaseMemo.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeCommonParams: FfiConverterRustBuffer<CommonParams?> {
    override fun read(buf: ByteBuffer): CommonParams? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeCommonParams.read(buf)
    }

    override fun allocationSize(value: CommonParams?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeCommonParams.allocationSize(value)
        }
    }

    override fun write(value: CommonParams?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeCommonParams.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeComposeAccount: FfiConverterRustBuffer<ComposeAccount?> {
    override fun read(buf: ByteBuffer): ComposeAccount? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeComposeAccount.read(buf)
    }

    override fun allocationSize(value: ComposeAccount?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeComposeAccount.allocationSize(value)
        }
    }

    override fun write(value: ComposeAccount?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeComposeAccount.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeCreateCjitOptions: FfiConverterRustBuffer<CreateCjitOptions?> {
    override fun read(buf: ByteBuffer): CreateCjitOptions? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeCreateCjitOptions.read(buf)
    }

    override fun allocationSize(value: CreateCjitOptions?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeCreateCjitOptions.allocationSize(value)
        }
    }

    override fun write(value: CreateCjitOptions?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeCreateCjitOptions.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeCreateOrderOptions: FfiConverterRustBuffer<CreateOrderOptions?> {
    override fun read(buf: ByteBuffer): CreateOrderOptions? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeCreateOrderOptions.read(buf)
    }

    override fun allocationSize(value: CreateOrderOptions?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeCreateOrderOptions.allocationSize(value)
        }
    }

    override fun write(value: CreateOrderOptions?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeCreateOrderOptions.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeDeviceParams: FfiConverterRustBuffer<DeviceParams?> {
    override fun read(buf: ByteBuffer): DeviceParams? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeDeviceParams.read(buf)
    }

    override fun allocationSize(value: DeviceParams?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeDeviceParams.allocationSize(value)
        }
    }

    override fun write(value: DeviceParams?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeDeviceParams.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeIBtBolt11Invoice: FfiConverterRustBuffer<IBtBolt11Invoice?> {
    override fun read(buf: ByteBuffer): IBtBolt11Invoice? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeIBtBolt11Invoice.read(buf)
    }

    override fun allocationSize(value: IBtBolt11Invoice?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeIBtBolt11Invoice.allocationSize(value)
        }
    }

    override fun write(value: IBtBolt11Invoice?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeIBtBolt11Invoice.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeIBtChannel: FfiConverterRustBuffer<IBtChannel?> {
    override fun read(buf: ByteBuffer): IBtChannel? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeIBtChannel.read(buf)
    }

    override fun allocationSize(value: IBtChannel?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeIBtChannel.allocationSize(value)
        }
    }

    override fun write(value: IBtChannel?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeIBtChannel.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeIBtChannelClose: FfiConverterRustBuffer<IBtChannelClose?> {
    override fun read(buf: ByteBuffer): IBtChannelClose? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeIBtChannelClose.read(buf)
    }

    override fun allocationSize(value: IBtChannelClose?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeIBtChannelClose.allocationSize(value)
        }
    }

    override fun write(value: IBtChannelClose?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeIBtChannelClose.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeIBtInfo: FfiConverterRustBuffer<IBtInfo?> {
    override fun read(buf: ByteBuffer): IBtInfo? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeIBtInfo.read(buf)
    }

    override fun allocationSize(value: IBtInfo?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeIBtInfo.allocationSize(value)
        }
    }

    override fun write(value: IBtInfo?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeIBtInfo.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeIBtOnchainTransactions: FfiConverterRustBuffer<IBtOnchainTransactions?> {
    override fun read(buf: ByteBuffer): IBtOnchainTransactions? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeIBtOnchainTransactions.read(buf)
    }

    override fun allocationSize(value: IBtOnchainTransactions?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeIBtOnchainTransactions.allocationSize(value)
        }
    }

    override fun write(value: IBtOnchainTransactions?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeIBtOnchainTransactions.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeIBtPayment: FfiConverterRustBuffer<IBtPayment?> {
    override fun read(buf: ByteBuffer): IBtPayment? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeIBtPayment.read(buf)
    }

    override fun allocationSize(value: IBtPayment?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeIBtPayment.allocationSize(value)
        }
    }

    override fun write(value: IBtPayment?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeIBtPayment.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeIDiscount: FfiConverterRustBuffer<IDiscount?> {
    override fun read(buf: ByteBuffer): IDiscount? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeIDiscount.read(buf)
    }

    override fun allocationSize(value: IDiscount?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeIDiscount.allocationSize(value)
        }
    }

    override fun write(value: IDiscount?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeIDiscount.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeIGiftBolt11Invoice: FfiConverterRustBuffer<IGiftBolt11Invoice?> {
    override fun read(buf: ByteBuffer): IGiftBolt11Invoice? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeIGiftBolt11Invoice.read(buf)
    }

    override fun allocationSize(value: IGiftBolt11Invoice?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeIGiftBolt11Invoice.allocationSize(value)
        }
    }

    override fun write(value: IGiftBolt11Invoice?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeIGiftBolt11Invoice.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeIGiftBtcAddress: FfiConverterRustBuffer<IGiftBtcAddress?> {
    override fun read(buf: ByteBuffer): IGiftBtcAddress? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeIGiftBtcAddress.read(buf)
    }

    override fun allocationSize(value: IGiftBtcAddress?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeIGiftBtcAddress.allocationSize(value)
        }
    }

    override fun write(value: IGiftBtcAddress?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeIGiftBtcAddress.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeIGiftCode: FfiConverterRustBuffer<IGiftCode?> {
    override fun read(buf: ByteBuffer): IGiftCode? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeIGiftCode.read(buf)
    }

    override fun allocationSize(value: IGiftCode?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeIGiftCode.allocationSize(value)
        }
    }

    override fun write(value: IGiftCode?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeIGiftCode.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeIGiftLspNode: FfiConverterRustBuffer<IGiftLspNode?> {
    override fun read(buf: ByteBuffer): IGiftLspNode? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeIGiftLspNode.read(buf)
    }

    override fun allocationSize(value: IGiftLspNode?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeIGiftLspNode.allocationSize(value)
        }
    }

    override fun write(value: IGiftLspNode?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeIGiftLspNode.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeIGiftOrder: FfiConverterRustBuffer<IGiftOrder?> {
    override fun read(buf: ByteBuffer): IGiftOrder? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeIGiftOrder.read(buf)
    }

    override fun allocationSize(value: IGiftOrder?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeIGiftOrder.allocationSize(value)
        }
    }

    override fun write(value: IGiftOrder?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeIGiftOrder.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeIGiftPayment: FfiConverterRustBuffer<IGiftPayment?> {
    override fun read(buf: ByteBuffer): IGiftPayment? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeIGiftPayment.read(buf)
    }

    override fun allocationSize(value: IGiftPayment?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeIGiftPayment.allocationSize(value)
        }
    }

    override fun write(value: IGiftPayment?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeIGiftPayment.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeILspNode: FfiConverterRustBuffer<ILspNode?> {
    override fun read(buf: ByteBuffer): ILspNode? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeILspNode.read(buf)
    }

    override fun allocationSize(value: ILspNode?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeILspNode.allocationSize(value)
        }
    }

    override fun write(value: ILspNode?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeILspNode.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeMultisigRedeemScriptType: FfiConverterRustBuffer<MultisigRedeemScriptType?> {
    override fun read(buf: ByteBuffer): MultisigRedeemScriptType? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeMultisigRedeemScriptType.read(buf)
    }

    override fun allocationSize(value: MultisigRedeemScriptType?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeMultisigRedeemScriptType.allocationSize(value)
        }
    }

    override fun write(value: MultisigRedeemScriptType?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeMultisigRedeemScriptType.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypePreActivityMetadata: FfiConverterRustBuffer<PreActivityMetadata?> {
    override fun read(buf: ByteBuffer): PreActivityMetadata? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypePreActivityMetadata.read(buf)
    }

    override fun allocationSize(value: PreActivityMetadata?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypePreActivityMetadata.allocationSize(value)
        }
    }

    override fun write(value: PreActivityMetadata?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypePreActivityMetadata.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeRefundMemo: FfiConverterRustBuffer<RefundMemo?> {
    override fun read(buf: ByteBuffer): RefundMemo? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeRefundMemo.read(buf)
    }

    override fun allocationSize(value: RefundMemo?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeRefundMemo.allocationSize(value)
        }
    }

    override fun write(value: RefundMemo?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeRefundMemo.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeTextMemo: FfiConverterRustBuffer<TextMemo?> {
    override fun read(buf: ByteBuffer): TextMemo? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeTextMemo.read(buf)
    }

    override fun allocationSize(value: TextMemo?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeTextMemo.allocationSize(value)
        }
    }

    override fun write(value: TextMemo?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeTextMemo.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeUnlockPath: FfiConverterRustBuffer<UnlockPath?> {
    override fun read(buf: ByteBuffer): UnlockPath? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeUnlockPath.read(buf)
    }

    override fun allocationSize(value: UnlockPath?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeUnlockPath.allocationSize(value)
        }
    }

    override fun write(value: UnlockPath?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeUnlockPath.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeXrpMarker: FfiConverterRustBuffer<XrpMarker?> {
    override fun read(buf: ByteBuffer): XrpMarker? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeXrpMarker.read(buf)
    }

    override fun allocationSize(value: XrpMarker?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeXrpMarker.allocationSize(value)
        }
    }

    override fun write(value: XrpMarker?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeXrpMarker.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeAccountInfoDetails: FfiConverterRustBuffer<AccountInfoDetails?> {
    override fun read(buf: ByteBuffer): AccountInfoDetails? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeAccountInfoDetails.read(buf)
    }

    override fun allocationSize(value: AccountInfoDetails?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeAccountInfoDetails.allocationSize(value)
        }
    }

    override fun write(value: AccountInfoDetails?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeAccountInfoDetails.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeActivity: FfiConverterRustBuffer<Activity?> {
    override fun read(buf: ByteBuffer): Activity? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeActivity.read(buf)
    }

    override fun allocationSize(value: Activity?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeActivity.allocationSize(value)
        }
    }

    override fun write(value: Activity?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeActivity.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeActivityFilter: FfiConverterRustBuffer<ActivityFilter?> {
    override fun read(buf: ByteBuffer): ActivityFilter? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeActivityFilter.read(buf)
    }

    override fun allocationSize(value: ActivityFilter?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeActivityFilter.allocationSize(value)
        }
    }

    override fun write(value: ActivityFilter?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeActivityFilter.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeAmountUnit: FfiConverterRustBuffer<AmountUnit?> {
    override fun read(buf: ByteBuffer): AmountUnit? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeAmountUnit.read(buf)
    }

    override fun allocationSize(value: AmountUnit?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeAmountUnit.allocationSize(value)
        }
    }

    override fun write(value: AmountUnit?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeAmountUnit.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeBtOrderState2: FfiConverterRustBuffer<BtOrderState2?> {
    override fun read(buf: ByteBuffer): BtOrderState2? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeBtOrderState2.read(buf)
    }

    override fun allocationSize(value: BtOrderState2?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeBtOrderState2.allocationSize(value)
        }
    }

    override fun write(value: BtOrderState2?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeBtOrderState2.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeBtPaymentState2: FfiConverterRustBuffer<BtPaymentState2?> {
    override fun read(buf: ByteBuffer): BtPaymentState2? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeBtPaymentState2.read(buf)
    }

    override fun allocationSize(value: BtPaymentState2?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeBtPaymentState2.allocationSize(value)
        }
    }

    override fun write(value: BtPaymentState2?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeBtPaymentState2.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeCJitStateEnum: FfiConverterRustBuffer<CJitStateEnum?> {
    override fun read(buf: ByteBuffer): CJitStateEnum? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeCJitStateEnum.read(buf)
    }

    override fun allocationSize(value: CJitStateEnum?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeCJitStateEnum.allocationSize(value)
        }
    }

    override fun write(value: CJitStateEnum?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeCJitStateEnum.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeDefaultAccountType: FfiConverterRustBuffer<DefaultAccountType?> {
    override fun read(buf: ByteBuffer): DefaultAccountType? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeDefaultAccountType.read(buf)
    }

    override fun allocationSize(value: DefaultAccountType?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeDefaultAccountType.allocationSize(value)
        }
    }

    override fun write(value: DefaultAccountType?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeDefaultAccountType.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeNetwork: FfiConverterRustBuffer<Network?> {
    override fun read(buf: ByteBuffer): Network? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeNetwork.read(buf)
    }

    override fun allocationSize(value: Network?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeNetwork.allocationSize(value)
        }
    }

    override fun write(value: Network?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeNetwork.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypePaymentType: FfiConverterRustBuffer<PaymentType?> {
    override fun read(buf: ByteBuffer): PaymentType? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypePaymentType.read(buf)
    }

    override fun allocationSize(value: PaymentType?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypePaymentType.allocationSize(value)
        }
    }

    override fun write(value: PaymentType?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypePaymentType.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeScriptType: FfiConverterRustBuffer<ScriptType?> {
    override fun read(buf: ByteBuffer): ScriptType? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeScriptType.read(buf)
    }

    override fun allocationSize(value: ScriptType?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeScriptType.allocationSize(value)
        }
    }

    override fun write(value: ScriptType?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeScriptType.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeSortDirection: FfiConverterRustBuffer<SortDirection?> {
    override fun read(buf: ByteBuffer): SortDirection? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeSortDirection.read(buf)
    }

    override fun allocationSize(value: SortDirection?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeSortDirection.allocationSize(value)
        }
    }

    override fun write(value: SortDirection?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeSortDirection.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeTokenFilter: FfiConverterRustBuffer<TokenFilter?> {
    override fun read(buf: ByteBuffer): TokenFilter? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeTokenFilter.read(buf)
    }

    override fun allocationSize(value: TokenFilter?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeTokenFilter.allocationSize(value)
        }
    }

    override fun write(value: TokenFilter?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeTokenFilter.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeTrezorEnvironment: FfiConverterRustBuffer<TrezorEnvironment?> {
    override fun read(buf: ByteBuffer): TrezorEnvironment? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeTrezorEnvironment.read(buf)
    }

    override fun allocationSize(value: TrezorEnvironment?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeTrezorEnvironment.allocationSize(value)
        }
    }

    override fun write(value: TrezorEnvironment?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeTrezorEnvironment.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeWordCount: FfiConverterRustBuffer<WordCount?> {
    override fun read(buf: ByteBuffer): WordCount? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeWordCount.read(buf)
    }

    override fun allocationSize(value: WordCount?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeWordCount.allocationSize(value)
        }
    }

    override fun write(value: WordCount?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeWordCount.write(value, buf)
        }
    }
}




public object FfiConverterOptionalSequenceUInt: FfiConverterRustBuffer<List<kotlin.UInt>?> {
    override fun read(buf: ByteBuffer): List<kotlin.UInt>? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterSequenceUInt.read(buf)
    }

    override fun allocationSize(value: List<kotlin.UInt>?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterSequenceUInt.allocationSize(value)
        }
    }

    override fun write(value: List<kotlin.UInt>?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterSequenceUInt.write(value, buf)
        }
    }
}




public object FfiConverterOptionalSequenceString: FfiConverterRustBuffer<List<kotlin.String>?> {
    override fun read(buf: ByteBuffer): List<kotlin.String>? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterSequenceString.read(buf)
    }

    override fun allocationSize(value: List<kotlin.String>?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterSequenceString.allocationSize(value)
        }
    }

    override fun write(value: List<kotlin.String>?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterSequenceString.write(value, buf)
        }
    }
}




public object FfiConverterOptionalSequenceTypeFeeLevel: FfiConverterRustBuffer<List<FeeLevel>?> {
    override fun read(buf: ByteBuffer): List<FeeLevel>? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterSequenceTypeFeeLevel.read(buf)
    }

    override fun allocationSize(value: List<FeeLevel>?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterSequenceTypeFeeLevel.allocationSize(value)
        }
    }

    override fun write(value: List<FeeLevel>?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterSequenceTypeFeeLevel.write(value, buf)
        }
    }
}




public object FfiConverterOptionalSequenceTypeHDNodeType: FfiConverterRustBuffer<List<HdNodeType>?> {
    override fun read(buf: ByteBuffer): List<HdNodeType>? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterSequenceTypeHDNodeType.read(buf)
    }

    override fun allocationSize(value: List<HdNodeType>?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterSequenceTypeHDNodeType.allocationSize(value)
        }
    }

    override fun write(value: List<HdNodeType>?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterSequenceTypeHDNodeType.write(value, buf)
        }
    }
}




public object FfiConverterOptionalSequenceTypeIManualRefund: FfiConverterRustBuffer<List<IManualRefund>?> {
    override fun read(buf: ByteBuffer): List<IManualRefund>? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterSequenceTypeIManualRefund.read(buf)
    }

    override fun allocationSize(value: List<IManualRefund>?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterSequenceTypeIManualRefund.allocationSize(value)
        }
    }

    override fun write(value: List<IManualRefund>?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterSequenceTypeIManualRefund.write(value, buf)
        }
    }
}




public object FfiConverterOptionalSequenceTypePaymentRequestMemo: FfiConverterRustBuffer<List<PaymentRequestMemo>?> {
    override fun read(buf: ByteBuffer): List<PaymentRequestMemo>? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterSequenceTypePaymentRequestMemo.read(buf)
    }

    override fun allocationSize(value: List<PaymentRequestMemo>?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterSequenceTypePaymentRequestMemo.allocationSize(value)
        }
    }

    override fun write(value: List<PaymentRequestMemo>?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterSequenceTypePaymentRequestMemo.write(value, buf)
        }
    }
}




public object FfiConverterOptionalSequenceTypePrecomposedInput: FfiConverterRustBuffer<List<PrecomposedInput>?> {
    override fun read(buf: ByteBuffer): List<PrecomposedInput>? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterSequenceTypePrecomposedInput.read(buf)
    }

    override fun allocationSize(value: List<PrecomposedInput>?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterSequenceTypePrecomposedInput.allocationSize(value)
        }
    }

    override fun write(value: List<PrecomposedInput>?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterSequenceTypePrecomposedInput.write(value, buf)
        }
    }
}




public object FfiConverterOptionalSequenceTypePrecomposedOutput: FfiConverterRustBuffer<List<PrecomposedOutput>?> {
    override fun read(buf: ByteBuffer): List<PrecomposedOutput>? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterSequenceTypePrecomposedOutput.read(buf)
    }

    override fun allocationSize(value: List<PrecomposedOutput>?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterSequenceTypePrecomposedOutput.allocationSize(value)
        }
    }

    override fun write(value: List<PrecomposedOutput>?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterSequenceTypePrecomposedOutput.write(value, buf)
        }
    }
}




public object FfiConverterOptionalSequenceTypeRefTransaction: FfiConverterRustBuffer<List<RefTransaction>?> {
    override fun read(buf: ByteBuffer): List<RefTransaction>? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterSequenceTypeRefTransaction.read(buf)
    }

    override fun allocationSize(value: List<RefTransaction>?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterSequenceTypeRefTransaction.allocationSize(value)
        }
    }

    override fun write(value: List<RefTransaction>?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterSequenceTypeRefTransaction.write(value, buf)
        }
    }
}




public object FfiConverterOptionalSequenceTypeTxAckPaymentRequest: FfiConverterRustBuffer<List<TxAckPaymentRequest>?> {
    override fun read(buf: ByteBuffer): List<TxAckPaymentRequest>? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterSequenceTypeTxAckPaymentRequest.read(buf)
    }

    override fun allocationSize(value: List<TxAckPaymentRequest>?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterSequenceTypeTxAckPaymentRequest.allocationSize(value)
        }
    }

    override fun write(value: List<TxAckPaymentRequest>?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterSequenceTypeTxAckPaymentRequest.write(value, buf)
        }
    }
}




public object FfiConverterOptionalMapStringString: FfiConverterRustBuffer<Map<kotlin.String, kotlin.String>?> {
    override fun read(buf: ByteBuffer): Map<kotlin.String, kotlin.String>? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterMapStringString.read(buf)
    }

    override fun allocationSize(value: Map<kotlin.String, kotlin.String>?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterMapStringString.allocationSize(value)
        }
    }

    override fun write(value: Map<kotlin.String, kotlin.String>?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterMapStringString.write(value, buf)
        }
    }
}




public object FfiConverterSequenceUInt: FfiConverterRustBuffer<List<kotlin.UInt>> {
    override fun read(buf: ByteBuffer): List<kotlin.UInt> {
        val len = buf.getInt()
        return List<kotlin.UInt>(len) {
            FfiConverterUInt.read(buf)
        }
    }

    override fun allocationSize(value: List<kotlin.UInt>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterUInt.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<kotlin.UInt>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterUInt.write(it, buf)
        }
    }
}




public object FfiConverterSequenceString: FfiConverterRustBuffer<List<kotlin.String>> {
    override fun read(buf: ByteBuffer): List<kotlin.String> {
        val len = buf.getInt()
        return List<kotlin.String>(len) {
            FfiConverterString.read(buf)
        }
    }

    override fun allocationSize(value: List<kotlin.String>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterString.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<kotlin.String>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterString.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeAccountUtxo: FfiConverterRustBuffer<List<AccountUtxo>> {
    override fun read(buf: ByteBuffer): List<AccountUtxo> {
        val len = buf.getInt()
        return List<AccountUtxo>(len) {
            FfiConverterTypeAccountUtxo.read(buf)
        }
    }

    override fun allocationSize(value: List<AccountUtxo>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeAccountUtxo.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<AccountUtxo>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeAccountUtxo.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeActivityTags: FfiConverterRustBuffer<List<ActivityTags>> {
    override fun read(buf: ByteBuffer): List<ActivityTags> {
        val len = buf.getInt()
        return List<ActivityTags>(len) {
            FfiConverterTypeActivityTags.read(buf)
        }
    }

    override fun allocationSize(value: List<ActivityTags>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeActivityTags.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<ActivityTags>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeActivityTags.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeAddressInfo: FfiConverterRustBuffer<List<AddressInfo>> {
    override fun read(buf: ByteBuffer): List<AddressInfo> {
        val len = buf.getInt()
        return List<AddressInfo>(len) {
            FfiConverterTypeAddressInfo.read(buf)
        }
    }

    override fun allocationSize(value: List<AddressInfo>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeAddressInfo.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<AddressInfo>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeAddressInfo.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeClosedChannelDetails: FfiConverterRustBuffer<List<ClosedChannelDetails>> {
    override fun read(buf: ByteBuffer): List<ClosedChannelDetails> {
        val len = buf.getInt()
        return List<ClosedChannelDetails>(len) {
            FfiConverterTypeClosedChannelDetails.read(buf)
        }
    }

    override fun allocationSize(value: List<ClosedChannelDetails>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeClosedChannelDetails.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<ClosedChannelDetails>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeClosedChannelDetails.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeFeeLevel: FfiConverterRustBuffer<List<FeeLevel>> {
    override fun read(buf: ByteBuffer): List<FeeLevel> {
        val len = buf.getInt()
        return List<FeeLevel>(len) {
            FfiConverterTypeFeeLevel.read(buf)
        }
    }

    override fun allocationSize(value: List<FeeLevel>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeFeeLevel.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<FeeLevel>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeFeeLevel.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeGetAddressResponse: FfiConverterRustBuffer<List<GetAddressResponse>> {
    override fun read(buf: ByteBuffer): List<GetAddressResponse> {
        val len = buf.getInt()
        return List<GetAddressResponse>(len) {
            FfiConverterTypeGetAddressResponse.read(buf)
        }
    }

    override fun allocationSize(value: List<GetAddressResponse>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeGetAddressResponse.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<GetAddressResponse>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeGetAddressResponse.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeHDNodePathType: FfiConverterRustBuffer<List<HdNodePathType>> {
    override fun read(buf: ByteBuffer): List<HdNodePathType> {
        val len = buf.getInt()
        return List<HdNodePathType>(len) {
            FfiConverterTypeHDNodePathType.read(buf)
        }
    }

    override fun allocationSize(value: List<HdNodePathType>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeHDNodePathType.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<HdNodePathType>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeHDNodePathType.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeHDNodeType: FfiConverterRustBuffer<List<HdNodeType>> {
    override fun read(buf: ByteBuffer): List<HdNodeType> {
        val len = buf.getInt()
        return List<HdNodeType>(len) {
            FfiConverterTypeHDNodeType.read(buf)
        }
    }

    override fun allocationSize(value: List<HdNodeType>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeHDNodeType.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<HdNodeType>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeHDNodeType.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeIBtOnchainTransaction: FfiConverterRustBuffer<List<IBtOnchainTransaction>> {
    override fun read(buf: ByteBuffer): List<IBtOnchainTransaction> {
        val len = buf.getInt()
        return List<IBtOnchainTransaction>(len) {
            FfiConverterTypeIBtOnchainTransaction.read(buf)
        }
    }

    override fun allocationSize(value: List<IBtOnchainTransaction>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeIBtOnchainTransaction.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<IBtOnchainTransaction>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeIBtOnchainTransaction.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeIBtOrder: FfiConverterRustBuffer<List<IBtOrder>> {
    override fun read(buf: ByteBuffer): List<IBtOrder> {
        val len = buf.getInt()
        return List<IBtOrder>(len) {
            FfiConverterTypeIBtOrder.read(buf)
        }
    }

    override fun allocationSize(value: List<IBtOrder>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeIBtOrder.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<IBtOrder>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeIBtOrder.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeICJitEntry: FfiConverterRustBuffer<List<IcJitEntry>> {
    override fun read(buf: ByteBuffer): List<IcJitEntry> {
        val len = buf.getInt()
        return List<IcJitEntry>(len) {
            FfiConverterTypeICJitEntry.read(buf)
        }
    }

    override fun allocationSize(value: List<IcJitEntry>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeICJitEntry.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<IcJitEntry>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeICJitEntry.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeILspNode: FfiConverterRustBuffer<List<ILspNode>> {
    override fun read(buf: ByteBuffer): List<ILspNode> {
        val len = buf.getInt()
        return List<ILspNode>(len) {
            FfiConverterTypeILspNode.read(buf)
        }
    }

    override fun allocationSize(value: List<ILspNode>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeILspNode.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<ILspNode>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeILspNode.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeIManualRefund: FfiConverterRustBuffer<List<IManualRefund>> {
    override fun read(buf: ByteBuffer): List<IManualRefund> {
        val len = buf.getInt()
        return List<IManualRefund>(len) {
            FfiConverterTypeIManualRefund.read(buf)
        }
    }

    override fun allocationSize(value: List<IManualRefund>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeIManualRefund.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<IManualRefund>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeIManualRefund.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeLightningActivity: FfiConverterRustBuffer<List<LightningActivity>> {
    override fun read(buf: ByteBuffer): List<LightningActivity> {
        val len = buf.getInt()
        return List<LightningActivity>(len) {
            FfiConverterTypeLightningActivity.read(buf)
        }
    }

    override fun allocationSize(value: List<LightningActivity>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeLightningActivity.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<LightningActivity>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeLightningActivity.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeOnchainActivity: FfiConverterRustBuffer<List<OnchainActivity>> {
    override fun read(buf: ByteBuffer): List<OnchainActivity> {
        val len = buf.getInt()
        return List<OnchainActivity>(len) {
            FfiConverterTypeOnchainActivity.read(buf)
        }
    }

    override fun allocationSize(value: List<OnchainActivity>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeOnchainActivity.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<OnchainActivity>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeOnchainActivity.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypePaymentRequestMemo: FfiConverterRustBuffer<List<PaymentRequestMemo>> {
    override fun read(buf: ByteBuffer): List<PaymentRequestMemo> {
        val len = buf.getInt()
        return List<PaymentRequestMemo>(len) {
            FfiConverterTypePaymentRequestMemo.read(buf)
        }
    }

    override fun allocationSize(value: List<PaymentRequestMemo>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypePaymentRequestMemo.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<PaymentRequestMemo>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypePaymentRequestMemo.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypePreActivityMetadata: FfiConverterRustBuffer<List<PreActivityMetadata>> {
    override fun read(buf: ByteBuffer): List<PreActivityMetadata> {
        val len = buf.getInt()
        return List<PreActivityMetadata>(len) {
            FfiConverterTypePreActivityMetadata.read(buf)
        }
    }

    override fun allocationSize(value: List<PreActivityMetadata>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypePreActivityMetadata.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<PreActivityMetadata>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypePreActivityMetadata.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypePrecomposedInput: FfiConverterRustBuffer<List<PrecomposedInput>> {
    override fun read(buf: ByteBuffer): List<PrecomposedInput> {
        val len = buf.getInt()
        return List<PrecomposedInput>(len) {
            FfiConverterTypePrecomposedInput.read(buf)
        }
    }

    override fun allocationSize(value: List<PrecomposedInput>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypePrecomposedInput.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<PrecomposedInput>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypePrecomposedInput.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypePrecomposedOutput: FfiConverterRustBuffer<List<PrecomposedOutput>> {
    override fun read(buf: ByteBuffer): List<PrecomposedOutput> {
        val len = buf.getInt()
        return List<PrecomposedOutput>(len) {
            FfiConverterTypePrecomposedOutput.read(buf)
        }
    }

    override fun allocationSize(value: List<PrecomposedOutput>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypePrecomposedOutput.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<PrecomposedOutput>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypePrecomposedOutput.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypePrecomposedTransaction: FfiConverterRustBuffer<List<PrecomposedTransaction>> {
    override fun read(buf: ByteBuffer): List<PrecomposedTransaction> {
        val len = buf.getInt()
        return List<PrecomposedTransaction>(len) {
            FfiConverterTypePrecomposedTransaction.read(buf)
        }
    }

    override fun allocationSize(value: List<PrecomposedTransaction>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypePrecomposedTransaction.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<PrecomposedTransaction>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypePrecomposedTransaction.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeRefTransaction: FfiConverterRustBuffer<List<RefTransaction>> {
    override fun read(buf: ByteBuffer): List<RefTransaction> {
        val len = buf.getInt()
        return List<RefTransaction>(len) {
            FfiConverterTypeRefTransaction.read(buf)
        }
    }

    override fun allocationSize(value: List<RefTransaction>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeRefTransaction.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<RefTransaction>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeRefTransaction.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeRefTxInput: FfiConverterRustBuffer<List<RefTxInput>> {
    override fun read(buf: ByteBuffer): List<RefTxInput> {
        val len = buf.getInt()
        return List<RefTxInput>(len) {
            FfiConverterTypeRefTxInput.read(buf)
        }
    }

    override fun allocationSize(value: List<RefTxInput>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeRefTxInput.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<RefTxInput>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeRefTxInput.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeRefTxOutput: FfiConverterRustBuffer<List<RefTxOutput>> {
    override fun read(buf: ByteBuffer): List<RefTxOutput> {
        val len = buf.getInt()
        return List<RefTxOutput>(len) {
            FfiConverterTypeRefTxOutput.read(buf)
        }
    }

    override fun allocationSize(value: List<RefTxOutput>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeRefTxOutput.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<RefTxOutput>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeRefTxOutput.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeTxAckPaymentRequest: FfiConverterRustBuffer<List<TxAckPaymentRequest>> {
    override fun read(buf: ByteBuffer): List<TxAckPaymentRequest> {
        val len = buf.getInt()
        return List<TxAckPaymentRequest>(len) {
            FfiConverterTypeTxAckPaymentRequest.read(buf)
        }
    }

    override fun allocationSize(value: List<TxAckPaymentRequest>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeTxAckPaymentRequest.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<TxAckPaymentRequest>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeTxAckPaymentRequest.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeTxInputType: FfiConverterRustBuffer<List<TxInputType>> {
    override fun read(buf: ByteBuffer): List<TxInputType> {
        val len = buf.getInt()
        return List<TxInputType>(len) {
            FfiConverterTypeTxInputType.read(buf)
        }
    }

    override fun allocationSize(value: List<TxInputType>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeTxInputType.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<TxInputType>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeTxInputType.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeTxOutputType: FfiConverterRustBuffer<List<TxOutputType>> {
    override fun read(buf: ByteBuffer): List<TxOutputType> {
        val len = buf.getInt()
        return List<TxOutputType>(len) {
            FfiConverterTypeTxOutputType.read(buf)
        }
    }

    override fun allocationSize(value: List<TxOutputType>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeTxOutputType.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<TxOutputType>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeTxOutputType.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeActivity: FfiConverterRustBuffer<List<Activity>> {
    override fun read(buf: ByteBuffer): List<Activity> {
        val len = buf.getInt()
        return List<Activity>(len) {
            FfiConverterTypeActivity.read(buf)
        }
    }

    override fun allocationSize(value: List<Activity>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeActivity.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<Activity>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeActivity.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeComposeOutput: FfiConverterRustBuffer<List<ComposeOutput>> {
    override fun read(buf: ByteBuffer): List<ComposeOutput> {
        val len = buf.getInt()
        return List<ComposeOutput>(len) {
            FfiConverterTypeComposeOutput.read(buf)
        }
    }

    override fun allocationSize(value: List<ComposeOutput>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeComposeOutput.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<ComposeOutput>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeComposeOutput.write(it, buf)
        }
    }
}



public object FfiConverterMapStringString: FfiConverterRustBuffer<Map<kotlin.String, kotlin.String>> {
    override fun read(buf: ByteBuffer): Map<kotlin.String, kotlin.String> {
        val len = buf.getInt()
        return buildMap<kotlin.String, kotlin.String>(len) {
            repeat(len) {
                val k = FfiConverterString.read(buf)
                val v = FfiConverterString.read(buf)
                this[k] = v
            }
        }
    }

    override fun allocationSize(value: Map<kotlin.String, kotlin.String>): ULong {
        val spaceForMapSize = 4UL
        val spaceForChildren = value.entries.sumOf { (k, v) ->
            FfiConverterString.allocationSize(k) +
            FfiConverterString.allocationSize(v)
        }
        return spaceForMapSize + spaceForChildren
    }

    override fun write(value: Map<kotlin.String, kotlin.String>, buf: ByteBuffer) {
        buf.putInt(value.size)
        // The parens on `(k, v)` here ensure we're calling the right method,
        // which is important for compatibility with older android devices.
        // Ref https://blog.danlew.net/2017/03/16/kotlin-puzzler-whose-line-is-it-anyways/
        value.forEach { (k, v) ->
            FfiConverterString.write(k, buf)
            FfiConverterString.write(v, buf)
        }
    }
}












@Throws(ActivityException::class)
public fun `activityWipeAll`() {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_activity_wipe_all(
            uniffiRustCallStatus,
        )
    }
}

@Throws(ActivityException::class)
public fun `addPreActivityMetadata`(`preActivityMetadata`: PreActivityMetadata) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_add_pre_activity_metadata(
            FfiConverterTypePreActivityMetadata.lower(`preActivityMetadata`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(ActivityException::class)
public fun `addPreActivityMetadataTags`(`paymentId`: kotlin.String, `tags`: List<kotlin.String>) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_add_pre_activity_metadata_tags(
            FfiConverterString.lower(`paymentId`),
            FfiConverterSequenceString.lower(`tags`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(ActivityException::class)
public fun `addTags`(`activityId`: kotlin.String, `tags`: List<kotlin.String>) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_add_tags(
            FfiConverterString.lower(`activityId`),
            FfiConverterSequenceString.lower(`tags`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `blocktankRemoveAllCjitEntries`() {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_blocktank_remove_all_cjit_entries(
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_void(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_void(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_void(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_void(future) },
        // lift function
        { Unit },
        
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `blocktankRemoveAllOrders`() {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_blocktank_remove_all_orders(
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_void(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_void(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_void(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_void(future) },
        // lift function
        { Unit },
        
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `blocktankWipeAll`() {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_blocktank_wipe_all(
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_void(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_void(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_void(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_void(future) },
        // lift function
        { Unit },
        
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(LnurlException::class)
public fun `createChannelRequestUrl`(`k1`: kotlin.String, `callback`: kotlin.String, `localNodeId`: kotlin.String, `isPrivate`: kotlin.Boolean, `cancel`: kotlin.Boolean): kotlin.String {
    return FfiConverterString.lift(uniffiRustCallWithError(LnurlExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_create_channel_request_url(
            FfiConverterString.lower(`k1`),
            FfiConverterString.lower(`callback`),
            FfiConverterString.lower(`localNodeId`),
            FfiConverterBoolean.lower(`isPrivate`),
            FfiConverterBoolean.lower(`cancel`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `createCjitEntry`(`channelSizeSat`: kotlin.ULong, `invoiceSat`: kotlin.ULong, `invoiceDescription`: kotlin.String, `nodeId`: kotlin.String, `channelExpiryWeeks`: kotlin.UInt, `options`: CreateCjitOptions?): IcJitEntry {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_create_cjit_entry(
            FfiConverterULong.lower(`channelSizeSat`),
            FfiConverterULong.lower(`invoiceSat`),
            FfiConverterString.lower(`invoiceDescription`),
            FfiConverterString.lower(`nodeId`),
            FfiConverterUInt.lower(`channelExpiryWeeks`),
            FfiConverterOptionalTypeCreateCjitOptions.lower(`options`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeICJitEntry.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `createOrder`(`lspBalanceSat`: kotlin.ULong, `channelExpiryWeeks`: kotlin.UInt, `options`: CreateOrderOptions?): IBtOrder {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_create_order(
            FfiConverterULong.lower(`lspBalanceSat`),
            FfiConverterUInt.lower(`channelExpiryWeeks`),
            FfiConverterOptionalTypeCreateOrderOptions.lower(`options`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeIBtOrder.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(LnurlException::class)
public fun `createWithdrawCallbackUrl`(`k1`: kotlin.String, `callback`: kotlin.String, `paymentRequest`: kotlin.String): kotlin.String {
    return FfiConverterString.lift(uniffiRustCallWithError(LnurlExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_create_withdraw_callback_url(
            FfiConverterString.lower(`k1`),
            FfiConverterString.lower(`callback`),
            FfiConverterString.lower(`paymentRequest`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(DecodingException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `decode`(`invoice`: kotlin.String): Scanner {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_decode(
            FfiConverterString.lower(`invoice`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeScanner.lift(it) },
        // Error FFI converter
        DecodingExceptionErrorHandler,
    )
}

@Throws(ActivityException::class)
public fun `deleteActivityById`(`activityId`: kotlin.String): kotlin.Boolean {
    return FfiConverterBoolean.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_delete_activity_by_id(
            FfiConverterString.lower(`activityId`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(ActivityException::class)
public fun `deletePreActivityMetadata`(`paymentId`: kotlin.String) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_delete_pre_activity_metadata(
            FfiConverterString.lower(`paymentId`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(AddressException::class)
public fun `deriveBitcoinAddress`(`mnemonicPhrase`: kotlin.String, `derivationPathStr`: kotlin.String?, `network`: Network?, `bip39Passphrase`: kotlin.String?): GetAddressResponse {
    return FfiConverterTypeGetAddressResponse.lift(uniffiRustCallWithError(AddressExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_derive_bitcoin_address(
            FfiConverterString.lower(`mnemonicPhrase`),
            FfiConverterOptionalString.lower(`derivationPathStr`),
            FfiConverterOptionalTypeNetwork.lower(`network`),
            FfiConverterOptionalString.lower(`bip39Passphrase`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(AddressException::class)
public fun `deriveBitcoinAddresses`(`mnemonicPhrase`: kotlin.String, `derivationPathStr`: kotlin.String?, `network`: Network?, `bip39Passphrase`: kotlin.String?, `isChange`: kotlin.Boolean?, `startIndex`: kotlin.UInt?, `count`: kotlin.UInt?): GetAddressesResponse {
    return FfiConverterTypeGetAddressesResponse.lift(uniffiRustCallWithError(AddressExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_derive_bitcoin_addresses(
            FfiConverterString.lower(`mnemonicPhrase`),
            FfiConverterOptionalString.lower(`derivationPathStr`),
            FfiConverterOptionalTypeNetwork.lower(`network`),
            FfiConverterOptionalString.lower(`bip39Passphrase`),
            FfiConverterOptionalBoolean.lower(`isChange`),
            FfiConverterOptionalUInt.lower(`startIndex`),
            FfiConverterOptionalUInt.lower(`count`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(AddressException::class)
public fun `derivePrivateKey`(`mnemonicPhrase`: kotlin.String, `derivationPathStr`: kotlin.String?, `network`: Network?, `bip39Passphrase`: kotlin.String?): kotlin.String {
    return FfiConverterString.lift(uniffiRustCallWithError(AddressExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_derive_private_key(
            FfiConverterString.lower(`mnemonicPhrase`),
            FfiConverterOptionalString.lower(`derivationPathStr`),
            FfiConverterOptionalTypeNetwork.lower(`network`),
            FfiConverterOptionalString.lower(`bip39Passphrase`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(AddressException::class)
public fun `entropyToMnemonic`(`entropy`: kotlin.ByteArray): kotlin.String {
    return FfiConverterString.lift(uniffiRustCallWithError(AddressExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_entropy_to_mnemonic(
            FfiConverterByteArray.lower(`entropy`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `estimateOrderFee`(`lspBalanceSat`: kotlin.ULong, `channelExpiryWeeks`: kotlin.UInt, `options`: CreateOrderOptions?): IBtEstimateFeeResponse {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_estimate_order_fee(
            FfiConverterULong.lower(`lspBalanceSat`),
            FfiConverterUInt.lower(`channelExpiryWeeks`),
            FfiConverterOptionalTypeCreateOrderOptions.lower(`options`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeIBtEstimateFeeResponse.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `estimateOrderFeeFull`(`lspBalanceSat`: kotlin.ULong, `channelExpiryWeeks`: kotlin.UInt, `options`: CreateOrderOptions?): IBtEstimateFeeResponse2 {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_estimate_order_fee_full(
            FfiConverterULong.lower(`lspBalanceSat`),
            FfiConverterUInt.lower(`channelExpiryWeeks`),
            FfiConverterOptionalTypeCreateOrderOptions.lower(`options`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeIBtEstimateFeeResponse2.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(AddressException::class)
public fun `generateMnemonic`(`wordCount`: WordCount?): kotlin.String {
    return FfiConverterString.lift(uniffiRustCallWithError(AddressExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_generate_mnemonic(
            FfiConverterOptionalTypeWordCount.lower(`wordCount`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(ActivityException::class)
public fun `getActivities`(`filter`: ActivityFilter?, `txType`: PaymentType?, `tags`: List<kotlin.String>?, `search`: kotlin.String?, `minDate`: kotlin.ULong?, `maxDate`: kotlin.ULong?, `limit`: kotlin.UInt?, `sortDirection`: SortDirection?): List<Activity> {
    return FfiConverterSequenceTypeActivity.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_activities(
            FfiConverterOptionalTypeActivityFilter.lower(`filter`),
            FfiConverterOptionalTypePaymentType.lower(`txType`),
            FfiConverterOptionalSequenceString.lower(`tags`),
            FfiConverterOptionalString.lower(`search`),
            FfiConverterOptionalULong.lower(`minDate`),
            FfiConverterOptionalULong.lower(`maxDate`),
            FfiConverterOptionalUInt.lower(`limit`),
            FfiConverterOptionalTypeSortDirection.lower(`sortDirection`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(ActivityException::class)
public fun `getActivitiesByTag`(`tag`: kotlin.String, `limit`: kotlin.UInt?, `sortDirection`: SortDirection?): List<Activity> {
    return FfiConverterSequenceTypeActivity.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_activities_by_tag(
            FfiConverterString.lower(`tag`),
            FfiConverterOptionalUInt.lower(`limit`),
            FfiConverterOptionalTypeSortDirection.lower(`sortDirection`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(ActivityException::class)
public fun `getActivityById`(`activityId`: kotlin.String): Activity? {
    return FfiConverterOptionalTypeActivity.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_activity_by_id(
            FfiConverterString.lower(`activityId`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(ActivityException::class)
public fun `getAllActivitiesTags`(): List<ActivityTags> {
    return FfiConverterSequenceTypeActivityTags.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_all_activities_tags(
            uniffiRustCallStatus,
        )
    })
}

@Throws(ActivityException::class)
public fun `getAllClosedChannels`(`sortDirection`: SortDirection?): List<ClosedChannelDetails> {
    return FfiConverterSequenceTypeClosedChannelDetails.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_all_closed_channels(
            FfiConverterOptionalTypeSortDirection.lower(`sortDirection`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(ActivityException::class)
public fun `getAllPreActivityMetadata`(): List<PreActivityMetadata> {
    return FfiConverterSequenceTypePreActivityMetadata.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_all_pre_activity_metadata(
            uniffiRustCallStatus,
        )
    })
}

@Throws(ActivityException::class)
public fun `getAllUniqueTags`(): List<kotlin.String> {
    return FfiConverterSequenceString.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_all_unique_tags(
            uniffiRustCallStatus,
        )
    })
}

public fun `getBip39Suggestions`(`partialWord`: kotlin.String, `limit`: kotlin.UInt): List<kotlin.String> {
    return FfiConverterSequenceString.lift(uniffiRustCall { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_bip39_suggestions(
            FfiConverterString.lower(`partialWord`),
            FfiConverterUInt.lower(`limit`),
            uniffiRustCallStatus,
        )
    })
}

public fun `getBip39Wordlist`(): List<kotlin.String> {
    return FfiConverterSequenceString.lift(uniffiRustCall { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_bip39_wordlist(
            uniffiRustCallStatus,
        )
    })
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `getCjitEntries`(`entryIds`: List<kotlin.String>?, `filter`: CJitStateEnum?, `refresh`: kotlin.Boolean): List<IcJitEntry> {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_get_cjit_entries(
            FfiConverterOptionalSequenceString.lower(`entryIds`),
            FfiConverterOptionalTypeCJitStateEnum.lower(`filter`),
            FfiConverterBoolean.lower(`refresh`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterSequenceTypeICJitEntry.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(ActivityException::class)
public fun `getClosedChannelById`(`channelId`: kotlin.String): ClosedChannelDetails? {
    return FfiConverterOptionalTypeClosedChannelDetails.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_closed_channel_by_id(
            FfiConverterString.lower(`channelId`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `getGift`(`giftId`: kotlin.String): IGift {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_get_gift(
            FfiConverterString.lower(`giftId`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeIGift.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `getInfo`(`refresh`: kotlin.Boolean?): IBtInfo? {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_get_info(
            FfiConverterOptionalBoolean.lower(`refresh`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterOptionalTypeIBtInfo.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(LnurlException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `getLnurlInvoice`(`address`: kotlin.String, `amountSatoshis`: kotlin.ULong): kotlin.String {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_get_lnurl_invoice(
            FfiConverterString.lower(`address`),
            FfiConverterULong.lower(`amountSatoshis`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterString.lift(it) },
        // Error FFI converter
        LnurlExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `getMinZeroConfTxFee`(`orderId`: kotlin.String): IBt0ConfMinTxFeeWindow {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_get_min_zero_conf_tx_fee(
            FfiConverterString.lower(`orderId`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeIBt0ConfMinTxFeeWindow.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `getOrders`(`orderIds`: List<kotlin.String>?, `filter`: BtOrderState2?, `refresh`: kotlin.Boolean): List<IBtOrder> {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_get_orders(
            FfiConverterOptionalSequenceString.lower(`orderIds`),
            FfiConverterOptionalTypeBtOrderState2.lower(`filter`),
            FfiConverterBoolean.lower(`refresh`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterSequenceTypeIBtOrder.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `getPayment`(`paymentId`: kotlin.String): IBtBolt11Invoice {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_get_payment(
            FfiConverterString.lower(`paymentId`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeIBtBolt11Invoice.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(ActivityException::class)
public fun `getPreActivityMetadata`(`searchKey`: kotlin.String, `searchByAddress`: kotlin.Boolean): PreActivityMetadata? {
    return FfiConverterOptionalTypePreActivityMetadata.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_pre_activity_metadata(
            FfiConverterString.lower(`searchKey`),
            FfiConverterBoolean.lower(`searchByAddress`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(ActivityException::class)
public fun `getTags`(`activityId`: kotlin.String): List<kotlin.String> {
    return FfiConverterSequenceString.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_tags(
            FfiConverterString.lower(`activityId`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `giftOrder`(`clientNodeId`: kotlin.String, `code`: kotlin.String): IGift {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_gift_order(
            FfiConverterString.lower(`clientNodeId`),
            FfiConverterString.lower(`code`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeIGift.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `giftPay`(`invoice`: kotlin.String): IGift {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_gift_pay(
            FfiConverterString.lower(`invoice`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeIGift.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(DbException::class)
public fun `initDb`(`basePath`: kotlin.String): kotlin.String {
    return FfiConverterString.lift(uniffiRustCallWithError(DbExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_init_db(
            FfiConverterString.lower(`basePath`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(ActivityException::class)
public fun `insertActivity`(`activity`: Activity) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_insert_activity(
            FfiConverterTypeActivity.lower(`activity`),
            uniffiRustCallStatus,
        )
    }
}

public fun `isValidBip39Word`(`word`: kotlin.String): kotlin.Boolean {
    return FfiConverterBoolean.lift(uniffiRustCall { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_is_valid_bip39_word(
            FfiConverterString.lower(`word`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(LnurlException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `lnurlAuth`(`domain`: kotlin.String, `k1`: kotlin.String, `callback`: kotlin.String, `bip32Mnemonic`: kotlin.String, `network`: Network?, `bip39Passphrase`: kotlin.String?): kotlin.String {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_lnurl_auth(
            FfiConverterString.lower(`domain`),
            FfiConverterString.lower(`k1`),
            FfiConverterString.lower(`callback`),
            FfiConverterString.lower(`bip32Mnemonic`),
            FfiConverterOptionalTypeNetwork.lower(`network`),
            FfiConverterOptionalString.lower(`bip39Passphrase`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterString.lift(it) },
        // Error FFI converter
        LnurlExceptionErrorHandler,
    )
}

@Throws(AddressException::class)
public fun `mnemonicToEntropy`(`mnemonicPhrase`: kotlin.String): kotlin.ByteArray {
    return FfiConverterByteArray.lift(uniffiRustCallWithError(AddressExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_mnemonic_to_entropy(
            FfiConverterString.lower(`mnemonicPhrase`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(AddressException::class)
public fun `mnemonicToSeed`(`mnemonicPhrase`: kotlin.String, `passphrase`: kotlin.String?): kotlin.ByteArray {
    return FfiConverterByteArray.lift(uniffiRustCallWithError(AddressExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_mnemonic_to_seed(
            FfiConverterString.lower(`mnemonicPhrase`),
            FfiConverterOptionalString.lower(`passphrase`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `openChannel`(`orderId`: kotlin.String, `connectionString`: kotlin.String): IBtOrder {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_open_channel(
            FfiConverterString.lower(`orderId`),
            FfiConverterString.lower(`connectionString`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeIBtOrder.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

/**
 * Refresh all active CJIT entries in the database with latest data from the LSP
 */
@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `refreshActiveCjitEntries`(): List<IcJitEntry> {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_refresh_active_cjit_entries(
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterSequenceTypeICJitEntry.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

/**
 * Refresh all active orders in the database with latest data from the LSP
 */
@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `refreshActiveOrders`(): List<IBtOrder> {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_refresh_active_orders(
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterSequenceTypeIBtOrder.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `registerDevice`(`deviceToken`: kotlin.String, `publicKey`: kotlin.String, `features`: List<kotlin.String>, `nodeId`: kotlin.String, `isoTimestamp`: kotlin.String, `signature`: kotlin.String, `isProduction`: kotlin.Boolean?, `customUrl`: kotlin.String?): kotlin.String {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_register_device(
            FfiConverterString.lower(`deviceToken`),
            FfiConverterString.lower(`publicKey`),
            FfiConverterSequenceString.lower(`features`),
            FfiConverterString.lower(`nodeId`),
            FfiConverterString.lower(`isoTimestamp`),
            FfiConverterString.lower(`signature`),
            FfiConverterOptionalBoolean.lower(`isProduction`),
            FfiConverterOptionalString.lower(`customUrl`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterString.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `regtestCloseChannel`(`fundingTxId`: kotlin.String, `vout`: kotlin.UInt, `forceCloseAfterS`: kotlin.ULong?): kotlin.String {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_regtest_close_channel(
            FfiConverterString.lower(`fundingTxId`),
            FfiConverterUInt.lower(`vout`),
            FfiConverterOptionalULong.lower(`forceCloseAfterS`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterString.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `regtestDeposit`(`address`: kotlin.String, `amountSat`: kotlin.ULong?): kotlin.String {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_regtest_deposit(
            FfiConverterString.lower(`address`),
            FfiConverterOptionalULong.lower(`amountSat`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterString.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `regtestGetPayment`(`paymentId`: kotlin.String): IBtBolt11Invoice {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_regtest_get_payment(
            FfiConverterString.lower(`paymentId`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeIBtBolt11Invoice.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `regtestMine`(`count`: kotlin.UInt?) {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_regtest_mine(
            FfiConverterOptionalUInt.lower(`count`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_void(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_void(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_void(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_void(future) },
        // lift function
        { Unit },
        
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `regtestPay`(`invoice`: kotlin.String, `amountSat`: kotlin.ULong?): kotlin.String {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_regtest_pay(
            FfiConverterString.lower(`invoice`),
            FfiConverterOptionalULong.lower(`amountSat`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterString.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(ActivityException::class)
public fun `removeClosedChannelById`(`channelId`: kotlin.String): kotlin.Boolean {
    return FfiConverterBoolean.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_remove_closed_channel_by_id(
            FfiConverterString.lower(`channelId`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(ActivityException::class)
public fun `removePreActivityMetadataTags`(`paymentId`: kotlin.String, `tags`: List<kotlin.String>) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_remove_pre_activity_metadata_tags(
            FfiConverterString.lower(`paymentId`),
            FfiConverterSequenceString.lower(`tags`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(ActivityException::class)
public fun `removeTags`(`activityId`: kotlin.String, `tags`: List<kotlin.String>) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_remove_tags(
            FfiConverterString.lower(`activityId`),
            FfiConverterSequenceString.lower(`tags`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(ActivityException::class)
public fun `resetPreActivityMetadataTags`(`paymentId`: kotlin.String) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_reset_pre_activity_metadata_tags(
            FfiConverterString.lower(`paymentId`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `testNotification`(`deviceToken`: kotlin.String, `secretMessage`: kotlin.String, `notificationType`: kotlin.String?, `customUrl`: kotlin.String?): kotlin.String {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_test_notification(
            FfiConverterString.lower(`deviceToken`),
            FfiConverterString.lower(`secretMessage`),
            FfiConverterOptionalString.lower(`notificationType`),
            FfiConverterOptionalString.lower(`customUrl`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterString.lift(it) },
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(TrezorConnectException::class)
public fun `trezorComposeTransaction`(`outputs`: List<ComposeOutput>, `coin`: kotlin.String, `callbackUrl`: kotlin.String, `requestId`: kotlin.String?, `trezorEnvironment`: TrezorEnvironment?, `push`: kotlin.Boolean?, `sequence`: kotlin.UInt?, `account`: ComposeAccount?, `feeLevels`: List<FeeLevel>?, `skipPermutation`: kotlin.Boolean?, `common`: CommonParams?): DeepLinkResult {
    return FfiConverterTypeDeepLinkResult.lift(uniffiRustCallWithError(TrezorConnectExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_compose_transaction(
            FfiConverterSequenceTypeComposeOutput.lower(`outputs`),
            FfiConverterString.lower(`coin`),
            FfiConverterString.lower(`callbackUrl`),
            FfiConverterOptionalString.lower(`requestId`),
            FfiConverterOptionalTypeTrezorEnvironment.lower(`trezorEnvironment`),
            FfiConverterOptionalBoolean.lower(`push`),
            FfiConverterOptionalUInt.lower(`sequence`),
            FfiConverterOptionalTypeComposeAccount.lower(`account`),
            FfiConverterOptionalSequenceTypeFeeLevel.lower(`feeLevels`),
            FfiConverterOptionalBoolean.lower(`skipPermutation`),
            FfiConverterOptionalTypeCommonParams.lower(`common`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(TrezorConnectException::class)
public fun `trezorGetAccountInfo`(`coin`: kotlin.String, `callbackUrl`: kotlin.String, `requestId`: kotlin.String?, `trezorEnvironment`: TrezorEnvironment?, `path`: kotlin.String?, `descriptor`: kotlin.String?, `details`: AccountInfoDetails?, `tokens`: TokenFilter?, `page`: kotlin.UInt?, `pageSize`: kotlin.UInt?, `from`: kotlin.UInt?, `to`: kotlin.UInt?, `gap`: kotlin.UInt?, `contractFilter`: kotlin.String?, `marker`: XrpMarker?, `defaultAccountType`: DefaultAccountType?, `suppressBackupWarning`: kotlin.Boolean?, `common`: CommonParams?): DeepLinkResult {
    return FfiConverterTypeDeepLinkResult.lift(uniffiRustCallWithError(TrezorConnectExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_get_account_info(
            FfiConverterString.lower(`coin`),
            FfiConverterString.lower(`callbackUrl`),
            FfiConverterOptionalString.lower(`requestId`),
            FfiConverterOptionalTypeTrezorEnvironment.lower(`trezorEnvironment`),
            FfiConverterOptionalString.lower(`path`),
            FfiConverterOptionalString.lower(`descriptor`),
            FfiConverterOptionalTypeAccountInfoDetails.lower(`details`),
            FfiConverterOptionalTypeTokenFilter.lower(`tokens`),
            FfiConverterOptionalUInt.lower(`page`),
            FfiConverterOptionalUInt.lower(`pageSize`),
            FfiConverterOptionalUInt.lower(`from`),
            FfiConverterOptionalUInt.lower(`to`),
            FfiConverterOptionalUInt.lower(`gap`),
            FfiConverterOptionalString.lower(`contractFilter`),
            FfiConverterOptionalTypeXrpMarker.lower(`marker`),
            FfiConverterOptionalTypeDefaultAccountType.lower(`defaultAccountType`),
            FfiConverterOptionalBoolean.lower(`suppressBackupWarning`),
            FfiConverterOptionalTypeCommonParams.lower(`common`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(TrezorConnectException::class)
public fun `trezorGetAddress`(`path`: kotlin.String, `callbackUrl`: kotlin.String, `requestId`: kotlin.String?, `trezorEnvironment`: TrezorEnvironment?, `address`: kotlin.String?, `showOnTrezor`: kotlin.Boolean?, `chunkify`: kotlin.Boolean?, `useEventListener`: kotlin.Boolean?, `coin`: kotlin.String?, `crossChain`: kotlin.Boolean?, `multisig`: MultisigRedeemScriptType?, `scriptType`: kotlin.String?, `unlockPath`: UnlockPath?, `common`: CommonParams?): DeepLinkResult {
    return FfiConverterTypeDeepLinkResult.lift(uniffiRustCallWithError(TrezorConnectExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_get_address(
            FfiConverterString.lower(`path`),
            FfiConverterString.lower(`callbackUrl`),
            FfiConverterOptionalString.lower(`requestId`),
            FfiConverterOptionalTypeTrezorEnvironment.lower(`trezorEnvironment`),
            FfiConverterOptionalString.lower(`address`),
            FfiConverterOptionalBoolean.lower(`showOnTrezor`),
            FfiConverterOptionalBoolean.lower(`chunkify`),
            FfiConverterOptionalBoolean.lower(`useEventListener`),
            FfiConverterOptionalString.lower(`coin`),
            FfiConverterOptionalBoolean.lower(`crossChain`),
            FfiConverterOptionalTypeMultisigRedeemScriptType.lower(`multisig`),
            FfiConverterOptionalString.lower(`scriptType`),
            FfiConverterOptionalTypeUnlockPath.lower(`unlockPath`),
            FfiConverterOptionalTypeCommonParams.lower(`common`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(TrezorConnectException::class)
public fun `trezorGetFeatures`(`callbackUrl`: kotlin.String, `requestId`: kotlin.String?, `trezorEnvironment`: TrezorEnvironment?): DeepLinkResult {
    return FfiConverterTypeDeepLinkResult.lift(uniffiRustCallWithError(TrezorConnectExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_get_features(
            FfiConverterString.lower(`callbackUrl`),
            FfiConverterOptionalString.lower(`requestId`),
            FfiConverterOptionalTypeTrezorEnvironment.lower(`trezorEnvironment`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(TrezorConnectException::class)
public fun `trezorHandleDeepLink`(`callbackUrl`: kotlin.String): TrezorResponsePayload {
    return FfiConverterTypeTrezorResponsePayload.lift(uniffiRustCallWithError(TrezorConnectExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_handle_deep_link(
            FfiConverterString.lower(`callbackUrl`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(TrezorConnectException::class)
public fun `trezorSignMessage`(`path`: kotlin.String, `message`: kotlin.String, `callbackUrl`: kotlin.String, `requestId`: kotlin.String?, `trezorEnvironment`: TrezorEnvironment?, `coin`: kotlin.String?, `hex`: kotlin.Boolean?, `noScriptType`: kotlin.Boolean?, `common`: CommonParams?): DeepLinkResult {
    return FfiConverterTypeDeepLinkResult.lift(uniffiRustCallWithError(TrezorConnectExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_sign_message(
            FfiConverterString.lower(`path`),
            FfiConverterString.lower(`message`),
            FfiConverterString.lower(`callbackUrl`),
            FfiConverterOptionalString.lower(`requestId`),
            FfiConverterOptionalTypeTrezorEnvironment.lower(`trezorEnvironment`),
            FfiConverterOptionalString.lower(`coin`),
            FfiConverterOptionalBoolean.lower(`hex`),
            FfiConverterOptionalBoolean.lower(`noScriptType`),
            FfiConverterOptionalTypeCommonParams.lower(`common`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(TrezorConnectException::class)
public fun `trezorSignTransaction`(`coin`: kotlin.String, `inputs`: List<TxInputType>, `outputs`: List<TxOutputType>, `callbackUrl`: kotlin.String, `requestId`: kotlin.String?, `trezorEnvironment`: TrezorEnvironment?, `refTxs`: List<RefTransaction>?, `paymentRequests`: List<TxAckPaymentRequest>?, `locktime`: kotlin.UInt?, `version`: kotlin.UInt?, `expiry`: kotlin.UInt?, `versionGroupId`: kotlin.UInt?, `overwintered`: kotlin.Boolean?, `timestamp`: kotlin.UInt?, `branchId`: kotlin.UInt?, `push`: kotlin.Boolean?, `amountUnit`: AmountUnit?, `unlockPath`: UnlockPath?, `serialize`: kotlin.Boolean?, `chunkify`: kotlin.Boolean?, `common`: CommonParams?): DeepLinkResult {
    return FfiConverterTypeDeepLinkResult.lift(uniffiRustCallWithError(TrezorConnectExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_sign_transaction(
            FfiConverterString.lower(`coin`),
            FfiConverterSequenceTypeTxInputType.lower(`inputs`),
            FfiConverterSequenceTypeTxOutputType.lower(`outputs`),
            FfiConverterString.lower(`callbackUrl`),
            FfiConverterOptionalString.lower(`requestId`),
            FfiConverterOptionalTypeTrezorEnvironment.lower(`trezorEnvironment`),
            FfiConverterOptionalSequenceTypeRefTransaction.lower(`refTxs`),
            FfiConverterOptionalSequenceTypeTxAckPaymentRequest.lower(`paymentRequests`),
            FfiConverterOptionalUInt.lower(`locktime`),
            FfiConverterOptionalUInt.lower(`version`),
            FfiConverterOptionalUInt.lower(`expiry`),
            FfiConverterOptionalUInt.lower(`versionGroupId`),
            FfiConverterOptionalBoolean.lower(`overwintered`),
            FfiConverterOptionalUInt.lower(`timestamp`),
            FfiConverterOptionalUInt.lower(`branchId`),
            FfiConverterOptionalBoolean.lower(`push`),
            FfiConverterOptionalTypeAmountUnit.lower(`amountUnit`),
            FfiConverterOptionalTypeUnlockPath.lower(`unlockPath`),
            FfiConverterOptionalBoolean.lower(`serialize`),
            FfiConverterOptionalBoolean.lower(`chunkify`),
            FfiConverterOptionalTypeCommonParams.lower(`common`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(TrezorConnectException::class)
public fun `trezorVerifyMessage`(`address`: kotlin.String, `signature`: kotlin.String, `message`: kotlin.String, `coin`: kotlin.String, `callbackUrl`: kotlin.String, `requestId`: kotlin.String?, `trezorEnvironment`: TrezorEnvironment?, `hex`: kotlin.Boolean?, `common`: CommonParams?): DeepLinkResult {
    return FfiConverterTypeDeepLinkResult.lift(uniffiRustCallWithError(TrezorConnectExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_verify_message(
            FfiConverterString.lower(`address`),
            FfiConverterString.lower(`signature`),
            FfiConverterString.lower(`message`),
            FfiConverterString.lower(`coin`),
            FfiConverterString.lower(`callbackUrl`),
            FfiConverterOptionalString.lower(`requestId`),
            FfiConverterOptionalTypeTrezorEnvironment.lower(`trezorEnvironment`),
            FfiConverterOptionalBoolean.lower(`hex`),
            FfiConverterOptionalTypeCommonParams.lower(`common`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(ActivityException::class)
public fun `updateActivity`(`activityId`: kotlin.String, `activity`: Activity) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_update_activity(
            FfiConverterString.lower(`activityId`),
            FfiConverterTypeActivity.lower(`activity`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `updateBlocktankUrl`(`newUrl`: kotlin.String) {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_update_blocktank_url(
            FfiConverterString.lower(`newUrl`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_void(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_void(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_void(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_void(future) },
        // lift function
        { Unit },
        
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(ActivityException::class)
public fun `upsertActivities`(`activities`: List<Activity>) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_upsert_activities(
            FfiConverterSequenceTypeActivity.lower(`activities`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(ActivityException::class)
public fun `upsertActivity`(`activity`: Activity) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_upsert_activity(
            FfiConverterTypeActivity.lower(`activity`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `upsertCjitEntries`(`entries`: List<IcJitEntry>) {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_upsert_cjit_entries(
            FfiConverterSequenceTypeICJitEntry.lower(`entries`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_void(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_void(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_void(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_void(future) },
        // lift function
        { Unit },
        
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(ActivityException::class)
public fun `upsertClosedChannel`(`channel`: ClosedChannelDetails) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_upsert_closed_channel(
            FfiConverterTypeClosedChannelDetails.lower(`channel`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(ActivityException::class)
public fun `upsertClosedChannels`(`channels`: List<ClosedChannelDetails>) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_upsert_closed_channels(
            FfiConverterSequenceTypeClosedChannelDetails.lower(`channels`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `upsertInfo`(`info`: IBtInfo) {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_upsert_info(
            FfiConverterTypeIBtInfo.lower(`info`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_void(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_void(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_void(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_void(future) },
        // lift function
        { Unit },
        
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(ActivityException::class)
public fun `upsertLightningActivities`(`activities`: List<LightningActivity>) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_upsert_lightning_activities(
            FfiConverterSequenceTypeLightningActivity.lower(`activities`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(ActivityException::class)
public fun `upsertOnchainActivities`(`activities`: List<OnchainActivity>) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_upsert_onchain_activities(
            FfiConverterSequenceTypeOnchainActivity.lower(`activities`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(BlocktankException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `upsertOrders`(`orders`: List<IBtOrder>) {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_upsert_orders(
            FfiConverterSequenceTypeIBtOrder.lower(`orders`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_void(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_void(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_void(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_void(future) },
        // lift function
        { Unit },
        
        // Error FFI converter
        BlocktankExceptionErrorHandler,
    )
}

@Throws(ActivityException::class)
public fun `upsertPreActivityMetadata`(`preActivityMetadata`: List<PreActivityMetadata>) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_upsert_pre_activity_metadata(
            FfiConverterSequenceTypePreActivityMetadata.lower(`preActivityMetadata`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(ActivityException::class)
public fun `upsertTags`(`activityTags`: List<ActivityTags>) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_upsert_tags(
            FfiConverterSequenceTypeActivityTags.lower(`activityTags`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(AddressException::class)
public fun `validateBitcoinAddress`(`address`: kotlin.String): ValidationResult {
    return FfiConverterTypeValidationResult.lift(uniffiRustCallWithError(AddressExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_validate_bitcoin_address(
            FfiConverterString.lower(`address`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(AddressException::class)
public fun `validateMnemonic`(`mnemonicPhrase`: kotlin.String) {
    uniffiRustCallWithError(AddressExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_validate_mnemonic(
            FfiConverterString.lower(`mnemonicPhrase`),
            uniffiRustCallStatus,
        )
    }
}

@Throws(ActivityException::class)
public fun `wipeAllClosedChannels`() {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_wipe_all_closed_channels(
            uniffiRustCallStatus,
        )
    }
}

@Throws(DbException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `wipeAllDatabases`(): kotlin.String {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_wipe_all_databases(
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterString.lift(it) },
        // Error FFI converter
        DbExceptionErrorHandler,
    )
}


// Async support

internal const val UNIFFI_RUST_FUTURE_POLL_READY = 0.toByte()
internal const val UNIFFI_RUST_FUTURE_POLL_MAYBE_READY = 1.toByte()

internal val uniffiContinuationHandleMap = UniffiHandleMap<CancellableContinuation<Byte>>()

// FFI type for Rust future continuations
internal suspend fun<T, F, E: kotlin.Exception> uniffiRustCallAsync(
    rustFuture: Long,
    pollFunc: (Long, UniffiRustFutureContinuationCallback, Long) -> Unit,
    completeFunc: (Long, UniffiRustCallStatus) -> F,
    freeFunc: (Long) -> Unit,
    cancelFunc: (Long) -> Unit,
    liftFunc: (F) -> T,
    errorHandler: UniffiRustCallStatusErrorHandler<E>
): T {
    return withContext(Dispatchers.IO) {
        try {
            do {
                val pollResult = suspendCancellableCoroutine<Byte> { continuation ->
                    val handle = uniffiContinuationHandleMap.insert(continuation)
                    continuation.invokeOnCancellation {
                        cancelFunc(rustFuture)
                    }
                    pollFunc(
                        rustFuture,
                        uniffiRustFutureContinuationCallbackCallback,
                        handle
                    )
                }
            } while (pollResult != UNIFFI_RUST_FUTURE_POLL_READY);

            return@withContext liftFunc(
                uniffiRustCallWithError(errorHandler) { status -> completeFunc(rustFuture, status) }
            )
        } finally {
            freeFunc(rustFuture)
        }
    }
}

internal object uniffiRustFutureContinuationCallbackCallback: UniffiRustFutureContinuationCallback {
    override fun callback(data: Long, pollResult: Byte) {
        uniffiContinuationHandleMap.remove(data).resume(pollResult)
    }
}