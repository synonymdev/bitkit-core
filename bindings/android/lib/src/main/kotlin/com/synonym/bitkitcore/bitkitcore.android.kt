

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
import android.os.Build
import androidx.annotation.RequiresApi
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
internal interface UniffiCallbackInterfaceTrezorTransportCallbackMethod0: com.sun.jna.Callback {
    public fun callback(`uniffiHandle`: Long,`uniffiOutReturn`: RustBuffer,uniffiCallStatus: UniffiRustCallStatus,)
}
internal interface UniffiCallbackInterfaceTrezorTransportCallbackMethod1: com.sun.jna.Callback {
    public fun callback(`uniffiHandle`: Long,`path`: RustBufferByValue,`uniffiOutReturn`: RustBuffer,uniffiCallStatus: UniffiRustCallStatus,)
}
internal interface UniffiCallbackInterfaceTrezorTransportCallbackMethod2: com.sun.jna.Callback {
    public fun callback(`uniffiHandle`: Long,`path`: RustBufferByValue,`uniffiOutReturn`: RustBuffer,uniffiCallStatus: UniffiRustCallStatus,)
}
internal interface UniffiCallbackInterfaceTrezorTransportCallbackMethod3: com.sun.jna.Callback {
    public fun callback(`uniffiHandle`: Long,`path`: RustBufferByValue,`uniffiOutReturn`: RustBuffer,uniffiCallStatus: UniffiRustCallStatus,)
}
internal interface UniffiCallbackInterfaceTrezorTransportCallbackMethod4: com.sun.jna.Callback {
    public fun callback(`uniffiHandle`: Long,`path`: RustBufferByValue,`data`: RustBufferByValue,`uniffiOutReturn`: RustBuffer,uniffiCallStatus: UniffiRustCallStatus,)
}
internal interface UniffiCallbackInterfaceTrezorTransportCallbackMethod5: com.sun.jna.Callback {
    public fun callback(`uniffiHandle`: Long,`path`: RustBufferByValue,`uniffiOutReturn`: IntByReference,uniffiCallStatus: UniffiRustCallStatus,)
}
internal interface UniffiCallbackInterfaceTrezorTransportCallbackMethod6: com.sun.jna.Callback {
    public fun callback(`uniffiHandle`: Long,`path`: RustBufferByValue,`messageType`: Short,`data`: RustBufferByValue,`uniffiOutReturn`: RustBuffer,uniffiCallStatus: UniffiRustCallStatus,)
}
internal interface UniffiCallbackInterfaceTrezorTransportCallbackMethod7: com.sun.jna.Callback {
    public fun callback(`uniffiHandle`: Long,`uniffiOutReturn`: RustBuffer,uniffiCallStatus: UniffiRustCallStatus,)
}
internal interface UniffiCallbackInterfaceTrezorTransportCallbackMethod8: com.sun.jna.Callback {
    public fun callback(`uniffiHandle`: Long,`deviceId`: RustBufferByValue,`credentialJson`: RustBufferByValue,`uniffiOutReturn`: ByteByReference,uniffiCallStatus: UniffiRustCallStatus,)
}
internal interface UniffiCallbackInterfaceTrezorTransportCallbackMethod9: com.sun.jna.Callback {
    public fun callback(`uniffiHandle`: Long,`deviceId`: RustBufferByValue,`uniffiOutReturn`: RustBuffer,uniffiCallStatus: UniffiRustCallStatus,)
}
internal interface UniffiCallbackInterfaceTrezorTransportCallbackMethod10: com.sun.jna.Callback {
    public fun callback(`uniffiHandle`: Long,`tag`: RustBufferByValue,`message`: RustBufferByValue,`uniffiOutReturn`: Pointer,uniffiCallStatus: UniffiRustCallStatus,)
}
internal interface UniffiCallbackInterfaceTrezorUiCallbackMethod0: com.sun.jna.Callback {
    public fun callback(`uniffiHandle`: Long,`uniffiOutReturn`: RustBuffer,uniffiCallStatus: UniffiRustCallStatus,)
}
internal interface UniffiCallbackInterfaceTrezorUiCallbackMethod1: com.sun.jna.Callback {
    public fun callback(`uniffiHandle`: Long,`onDevice`: Byte,`uniffiOutReturn`: RustBuffer,uniffiCallStatus: UniffiRustCallStatus,)
}
@Structure.FieldOrder("enumerateDevices", "openDevice", "closeDevice", "readChunk", "writeChunk", "getChunkSize", "callMessage", "getPairingCode", "saveThpCredential", "loadThpCredential", "logDebug", "uniffiFree")
internal open class UniffiVTableCallbackInterfaceTrezorTransportCallbackStruct(
    @JvmField public var `enumerateDevices`: UniffiCallbackInterfaceTrezorTransportCallbackMethod0?,
    @JvmField public var `openDevice`: UniffiCallbackInterfaceTrezorTransportCallbackMethod1?,
    @JvmField public var `closeDevice`: UniffiCallbackInterfaceTrezorTransportCallbackMethod2?,
    @JvmField public var `readChunk`: UniffiCallbackInterfaceTrezorTransportCallbackMethod3?,
    @JvmField public var `writeChunk`: UniffiCallbackInterfaceTrezorTransportCallbackMethod4?,
    @JvmField public var `getChunkSize`: UniffiCallbackInterfaceTrezorTransportCallbackMethod5?,
    @JvmField public var `callMessage`: UniffiCallbackInterfaceTrezorTransportCallbackMethod6?,
    @JvmField public var `getPairingCode`: UniffiCallbackInterfaceTrezorTransportCallbackMethod7?,
    @JvmField public var `saveThpCredential`: UniffiCallbackInterfaceTrezorTransportCallbackMethod8?,
    @JvmField public var `loadThpCredential`: UniffiCallbackInterfaceTrezorTransportCallbackMethod9?,
    @JvmField public var `logDebug`: UniffiCallbackInterfaceTrezorTransportCallbackMethod10?,
    @JvmField public var `uniffiFree`: UniffiCallbackInterfaceFree?,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `enumerateDevices` = null,
        
        `openDevice` = null,
        
        `closeDevice` = null,
        
        `readChunk` = null,
        
        `writeChunk` = null,
        
        `getChunkSize` = null,
        
        `callMessage` = null,
        
        `getPairingCode` = null,
        
        `saveThpCredential` = null,
        
        `loadThpCredential` = null,
        
        `logDebug` = null,
        
        `uniffiFree` = null,
        
    )

    internal class UniffiByValue(
        `enumerateDevices`: UniffiCallbackInterfaceTrezorTransportCallbackMethod0?,
        `openDevice`: UniffiCallbackInterfaceTrezorTransportCallbackMethod1?,
        `closeDevice`: UniffiCallbackInterfaceTrezorTransportCallbackMethod2?,
        `readChunk`: UniffiCallbackInterfaceTrezorTransportCallbackMethod3?,
        `writeChunk`: UniffiCallbackInterfaceTrezorTransportCallbackMethod4?,
        `getChunkSize`: UniffiCallbackInterfaceTrezorTransportCallbackMethod5?,
        `callMessage`: UniffiCallbackInterfaceTrezorTransportCallbackMethod6?,
        `getPairingCode`: UniffiCallbackInterfaceTrezorTransportCallbackMethod7?,
        `saveThpCredential`: UniffiCallbackInterfaceTrezorTransportCallbackMethod8?,
        `loadThpCredential`: UniffiCallbackInterfaceTrezorTransportCallbackMethod9?,
        `logDebug`: UniffiCallbackInterfaceTrezorTransportCallbackMethod10?,
        `uniffiFree`: UniffiCallbackInterfaceFree?,
    ): UniffiVTableCallbackInterfaceTrezorTransportCallback(`enumerateDevices`,`openDevice`,`closeDevice`,`readChunk`,`writeChunk`,`getChunkSize`,`callMessage`,`getPairingCode`,`saveThpCredential`,`loadThpCredential`,`logDebug`,`uniffiFree`,), Structure.ByValue
}

internal typealias UniffiVTableCallbackInterfaceTrezorTransportCallback = UniffiVTableCallbackInterfaceTrezorTransportCallbackStruct

internal fun UniffiVTableCallbackInterfaceTrezorTransportCallback.uniffiSetValue(other: UniffiVTableCallbackInterfaceTrezorTransportCallback) {
    `enumerateDevices` = other.`enumerateDevices`
    `openDevice` = other.`openDevice`
    `closeDevice` = other.`closeDevice`
    `readChunk` = other.`readChunk`
    `writeChunk` = other.`writeChunk`
    `getChunkSize` = other.`getChunkSize`
    `callMessage` = other.`callMessage`
    `getPairingCode` = other.`getPairingCode`
    `saveThpCredential` = other.`saveThpCredential`
    `loadThpCredential` = other.`loadThpCredential`
    `logDebug` = other.`logDebug`
    `uniffiFree` = other.`uniffiFree`
}
internal fun UniffiVTableCallbackInterfaceTrezorTransportCallback.uniffiSetValue(other: UniffiVTableCallbackInterfaceTrezorTransportCallbackUniffiByValue) {
    `enumerateDevices` = other.`enumerateDevices`
    `openDevice` = other.`openDevice`
    `closeDevice` = other.`closeDevice`
    `readChunk` = other.`readChunk`
    `writeChunk` = other.`writeChunk`
    `getChunkSize` = other.`getChunkSize`
    `callMessage` = other.`callMessage`
    `getPairingCode` = other.`getPairingCode`
    `saveThpCredential` = other.`saveThpCredential`
    `loadThpCredential` = other.`loadThpCredential`
    `logDebug` = other.`logDebug`
    `uniffiFree` = other.`uniffiFree`
}

internal typealias UniffiVTableCallbackInterfaceTrezorTransportCallbackUniffiByValue = UniffiVTableCallbackInterfaceTrezorTransportCallbackStruct.UniffiByValue
@Structure.FieldOrder("onPinRequest", "onPassphraseRequest", "uniffiFree")
internal open class UniffiVTableCallbackInterfaceTrezorUiCallbackStruct(
    @JvmField public var `onPinRequest`: UniffiCallbackInterfaceTrezorUiCallbackMethod0?,
    @JvmField public var `onPassphraseRequest`: UniffiCallbackInterfaceTrezorUiCallbackMethod1?,
    @JvmField public var `uniffiFree`: UniffiCallbackInterfaceFree?,
) : com.sun.jna.Structure() {
    internal constructor(): this(
        
        `onPinRequest` = null,
        
        `onPassphraseRequest` = null,
        
        `uniffiFree` = null,
        
    )

    internal class UniffiByValue(
        `onPinRequest`: UniffiCallbackInterfaceTrezorUiCallbackMethod0?,
        `onPassphraseRequest`: UniffiCallbackInterfaceTrezorUiCallbackMethod1?,
        `uniffiFree`: UniffiCallbackInterfaceFree?,
    ): UniffiVTableCallbackInterfaceTrezorUiCallback(`onPinRequest`,`onPassphraseRequest`,`uniffiFree`,), Structure.ByValue
}

internal typealias UniffiVTableCallbackInterfaceTrezorUiCallback = UniffiVTableCallbackInterfaceTrezorUiCallbackStruct

internal fun UniffiVTableCallbackInterfaceTrezorUiCallback.uniffiSetValue(other: UniffiVTableCallbackInterfaceTrezorUiCallback) {
    `onPinRequest` = other.`onPinRequest`
    `onPassphraseRequest` = other.`onPassphraseRequest`
    `uniffiFree` = other.`uniffiFree`
}
internal fun UniffiVTableCallbackInterfaceTrezorUiCallback.uniffiSetValue(other: UniffiVTableCallbackInterfaceTrezorUiCallbackUniffiByValue) {
    `onPinRequest` = other.`onPinRequest`
    `onPassphraseRequest` = other.`onPassphraseRequest`
    `uniffiFree` = other.`uniffiFree`
}

internal typealias UniffiVTableCallbackInterfaceTrezorUiCallbackUniffiByValue = UniffiVTableCallbackInterfaceTrezorUiCallbackStruct.UniffiByValue


























































































































































































































































































































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
        if (uniffi_bitkitcore_checksum_func_broadcast_sweep_transaction() != 43422.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_calculate_channel_liquidity_options() != 51013.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_check_sweepable_balances() != 64201.toShort()) {
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
        if (uniffi_bitkitcore_checksum_func_delete_transaction_details() != 21670.toShort()) {
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
        if (uniffi_bitkitcore_checksum_func_get_activity_by_tx_id() != 2520.toShort()) {
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
        if (uniffi_bitkitcore_checksum_func_get_all_transaction_details() != 36056.toShort()) {
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
        if (uniffi_bitkitcore_checksum_func_get_default_lsp_balance() != 35903.toShort()) {
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
        if (uniffi_bitkitcore_checksum_func_get_transaction_details() != 6118.toShort()) {
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
        if (uniffi_bitkitcore_checksum_func_is_address_used() != 64038.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_is_valid_bip39_word() != 31846.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_lnurl_auth() != 58593.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_mark_activity_as_seen() != 65086.toShort()) {
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
        if (uniffi_bitkitcore_checksum_func_prepare_sweep_transaction() != 18273.toShort()) {
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
        if (uniffi_bitkitcore_checksum_func_trezor_clear_credentials() != 41940.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_connect() != 6551.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_disconnect() != 48780.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_get_address() != 12910.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_get_connected_device() != 48383.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_get_device_fingerprint() != 20344.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_get_features() != 13970.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_get_public_key() != 13743.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_initialize() != 16053.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_is_ble_available() != 12897.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_is_connected() != 42092.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_is_initialized() != 59329.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_list_devices() != 32859.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_scan() != 54763.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_set_transport_callback() != 30209.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_set_ui_callback() != 52321.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_sign_message() != 2925.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_sign_tx() != 42467.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_sign_tx_from_psbt() != 18852.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_func_trezor_verify_message() != 50739.toShort()) {
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
        if (uniffi_bitkitcore_checksum_func_upsert_transaction_details() != 62351.toShort()) {
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
        if (uniffi_bitkitcore_checksum_func_wipe_all_transaction_details() != 65339.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_method_trezortransportcallback_enumerate_devices() != 18766.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_method_trezortransportcallback_open_device() != 44156.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_method_trezortransportcallback_close_device() != 47933.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_method_trezortransportcallback_read_chunk() != 7645.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_method_trezortransportcallback_write_chunk() != 55967.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_method_trezortransportcallback_get_chunk_size() != 4994.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_method_trezortransportcallback_call_message() != 19414.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_method_trezortransportcallback_get_pairing_code() != 43475.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_method_trezortransportcallback_save_thp_credential() != 16694.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_method_trezortransportcallback_load_thp_credential() != 48790.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_method_trezortransportcallback_log_debug() != 44848.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_method_trezoruicallback_on_pin_request() != 50474.toShort()) {
            throw RuntimeException("UniFFI API checksum mismatch: try cleaning and rebuilding your project")
        }
        if (uniffi_bitkitcore_checksum_method_trezoruicallback_on_passphrase_request() != 63487.toShort()) {
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
    external fun uniffi_bitkitcore_checksum_func_broadcast_sweep_transaction(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_calculate_channel_liquidity_options(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_check_sweepable_balances(
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
    external fun uniffi_bitkitcore_checksum_func_delete_transaction_details(
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
    external fun uniffi_bitkitcore_checksum_func_get_activity_by_tx_id(
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
    external fun uniffi_bitkitcore_checksum_func_get_all_transaction_details(
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
    external fun uniffi_bitkitcore_checksum_func_get_default_lsp_balance(
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
    external fun uniffi_bitkitcore_checksum_func_get_transaction_details(
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
    external fun uniffi_bitkitcore_checksum_func_is_address_used(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_is_valid_bip39_word(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_lnurl_auth(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_mark_activity_as_seen(
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
    external fun uniffi_bitkitcore_checksum_func_prepare_sweep_transaction(
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
    external fun uniffi_bitkitcore_checksum_func_trezor_clear_credentials(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_connect(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_disconnect(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_get_address(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_get_connected_device(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_get_device_fingerprint(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_get_features(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_get_public_key(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_initialize(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_is_ble_available(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_is_connected(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_is_initialized(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_list_devices(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_scan(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_set_transport_callback(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_set_ui_callback(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_sign_message(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_sign_tx(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_func_trezor_sign_tx_from_psbt(
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
    external fun uniffi_bitkitcore_checksum_func_upsert_transaction_details(
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
    external fun uniffi_bitkitcore_checksum_func_wipe_all_transaction_details(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_method_trezortransportcallback_enumerate_devices(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_method_trezortransportcallback_open_device(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_method_trezortransportcallback_close_device(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_method_trezortransportcallback_read_chunk(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_method_trezortransportcallback_write_chunk(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_method_trezortransportcallback_get_chunk_size(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_method_trezortransportcallback_call_message(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_method_trezortransportcallback_get_pairing_code(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_method_trezortransportcallback_save_thp_credential(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_method_trezortransportcallback_load_thp_credential(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_method_trezortransportcallback_log_debug(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_method_trezoruicallback_on_pin_request(
    ): Short
    @JvmStatic
    external fun uniffi_bitkitcore_checksum_method_trezoruicallback_on_passphrase_request(
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
        uniffiCallbackInterfaceTrezorTransportCallback.register(this)
        uniffiCallbackInterfaceTrezorUiCallback.register(this)
    }
    // The Cleaner for the whole library
    internal val CLEANER: UniffiCleaner by lazy {
        UniffiCleaner.create()
    }
    @JvmStatic
    external fun uniffi_bitkitcore_fn_clone_trezortransportcallback(
        `ptr`: Pointer?,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Pointer?
    @JvmStatic
    external fun uniffi_bitkitcore_fn_free_trezortransportcallback(
        `ptr`: Pointer?,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_init_callback_vtable_trezortransportcallback(
        `vtable`: UniffiVTableCallbackInterfaceTrezorTransportCallback,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_method_trezortransportcallback_enumerate_devices(
        `ptr`: Pointer?,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_method_trezortransportcallback_open_device(
        `ptr`: Pointer?,
        `path`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_method_trezortransportcallback_close_device(
        `ptr`: Pointer?,
        `path`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_method_trezortransportcallback_read_chunk(
        `ptr`: Pointer?,
        `path`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_method_trezortransportcallback_write_chunk(
        `ptr`: Pointer?,
        `path`: RustBufferByValue,
        `data`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_method_trezortransportcallback_get_chunk_size(
        `ptr`: Pointer?,
        `path`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Int
    @JvmStatic
    external fun uniffi_bitkitcore_fn_method_trezortransportcallback_call_message(
        `ptr`: Pointer?,
        `path`: RustBufferByValue,
        `messageType`: Short,
        `data`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_method_trezortransportcallback_get_pairing_code(
        `ptr`: Pointer?,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_method_trezortransportcallback_save_thp_credential(
        `ptr`: Pointer?,
        `deviceId`: RustBufferByValue,
        `credentialJson`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Byte
    @JvmStatic
    external fun uniffi_bitkitcore_fn_method_trezortransportcallback_load_thp_credential(
        `ptr`: Pointer?,
        `deviceId`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_method_trezortransportcallback_log_debug(
        `ptr`: Pointer?,
        `tag`: RustBufferByValue,
        `message`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_clone_trezoruicallback(
        `ptr`: Pointer?,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Pointer?
    @JvmStatic
    external fun uniffi_bitkitcore_fn_free_trezoruicallback(
        `ptr`: Pointer?,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_init_callback_vtable_trezoruicallback(
        `vtable`: UniffiVTableCallbackInterfaceTrezorUiCallback,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_method_trezoruicallback_on_pin_request(
        `ptr`: Pointer?,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_method_trezoruicallback_on_passphrase_request(
        `ptr`: Pointer?,
        `onDevice`: Byte,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
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
    external fun uniffi_bitkitcore_fn_func_broadcast_sweep_transaction(
        `psbt`: RustBufferByValue,
        `mnemonicPhrase`: RustBufferByValue,
        `network`: RustBufferByValue,
        `bip39Passphrase`: RustBufferByValue,
        `electrumUrl`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_calculate_channel_liquidity_options(
        `params`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): RustBufferByValue
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_check_sweepable_balances(
        `mnemonicPhrase`: RustBufferByValue,
        `network`: RustBufferByValue,
        `bip39Passphrase`: RustBufferByValue,
        `electrumUrl`: RustBufferByValue,
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
    external fun uniffi_bitkitcore_fn_func_delete_transaction_details(
        `txId`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Byte
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
    external fun uniffi_bitkitcore_fn_func_get_activity_by_tx_id(
        `txId`: RustBufferByValue,
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
    external fun uniffi_bitkitcore_fn_func_get_all_transaction_details(
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
    external fun uniffi_bitkitcore_fn_func_get_default_lsp_balance(
        `params`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Long
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
    external fun uniffi_bitkitcore_fn_func_get_transaction_details(
        `txId`: RustBufferByValue,
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
    external fun uniffi_bitkitcore_fn_func_is_address_used(
        `address`: RustBufferByValue,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Byte
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
    external fun uniffi_bitkitcore_fn_func_mark_activity_as_seen(
        `activityId`: RustBufferByValue,
        `seenAt`: Long,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
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
    external fun uniffi_bitkitcore_fn_func_prepare_sweep_transaction(
        `mnemonicPhrase`: RustBufferByValue,
        `network`: RustBufferByValue,
        `bip39Passphrase`: RustBufferByValue,
        `electrumUrl`: RustBufferByValue,
        `destinationAddress`: RustBufferByValue,
        `feeRateSatsPerVbyte`: RustBufferByValue,
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
    external fun uniffi_bitkitcore_fn_func_trezor_clear_credentials(
        `deviceId`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_connect(
        `deviceId`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_disconnect(
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_get_address(
        `params`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_get_connected_device(
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_get_device_fingerprint(
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_get_features(
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_get_public_key(
        `params`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_initialize(
        `credentialPath`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_is_ble_available(
        uniffiCallStatus: UniffiRustCallStatus,
    ): Byte
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_is_connected(
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_is_initialized(
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_list_devices(
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_scan(
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_set_transport_callback(
        `callback`: Pointer?,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_set_ui_callback(
        `callback`: Pointer?,
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_sign_message(
        `params`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_sign_tx(
        `params`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_sign_tx_from_psbt(
        `psbtBase64`: RustBufferByValue,
        `network`: RustBufferByValue,
    ): Long
    @JvmStatic
    external fun uniffi_bitkitcore_fn_func_trezor_verify_message(
        `params`: RustBufferByValue,
    ): Long
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
    external fun uniffi_bitkitcore_fn_func_upsert_transaction_details(
        `detailsList`: RustBufferByValue,
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
    external fun uniffi_bitkitcore_fn_func_wipe_all_transaction_details(
        uniffiCallStatus: UniffiRustCallStatus,
    ): Unit
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

internal const val IDX_CALLBACK_FREE = 0
// Callback return codes
internal const val UNIFFI_CALLBACK_SUCCESS = 0
internal const val UNIFFI_CALLBACK_ERROR = 1
internal const val UNIFFI_CALLBACK_UNEXPECTED_ERROR = 2

public abstract class FfiConverterCallbackInterface<CallbackInterface: Any>: FfiConverter<CallbackInterface, Long> {
    internal val handleMap = UniffiHandleMap<CallbackInterface>()

    internal fun drop(handle: Long) {
        handleMap.remove(handle)
    }

    override fun lift(value: Long): CallbackInterface {
        return handleMap.get(value)
    }

    override fun read(buf: ByteBuffer): CallbackInterface = lift(buf.getLong())

    override fun lower(value: CallbackInterface): Long = handleMap.insert(value)

    override fun allocationSize(value: CallbackInterface): ULong = 8UL

    override fun write(value: CallbackInterface, buf: ByteBuffer) {
        buf.putLong(lower(value))
    }
}
// The cleaner interface for Object finalization code to run.
// This is the entry point to any implementation that we're using.
//
// The cleaner registers disposables and returns cleanables, so now we are
// defining a `UniffiCleaner` with a `UniffiClenaer.Cleanable` to abstract the
// different implementations available at compile time.
public interface UniffiCleaner {
    public interface Cleanable {
        public fun clean()
    }

    public fun register(resource: Any, disposable: Disposable): UniffiCleaner.Cleanable

    public companion object
}
// The fallback Jna cleaner, which is available for both Android, and the JVM.
private class UniffiJnaCleaner : UniffiCleaner {
    private val cleaner = com.sun.jna.internal.Cleaner.getCleaner()

    override fun register(resource: Any, disposable: Disposable): UniffiCleaner.Cleanable =
        UniffiJnaCleanable(cleaner.register(resource, UniffiCleanerAction(disposable)))
}

private class UniffiJnaCleanable(
    private val cleanable: com.sun.jna.internal.Cleaner.Cleanable,
) : UniffiCleaner.Cleanable {
    override fun clean() = cleanable.clean()
}

private class UniffiCleanerAction(private val disposable: Disposable): Runnable {
    override fun run() {
        disposable.destroy()
    }
}

// The SystemCleaner, available from API Level 33.
// Some API Level 33 OSes do not support using it, so we require API Level 34.
@RequiresApi(Build.VERSION_CODES.UPSIDE_DOWN_CAKE)
private class AndroidSystemCleaner : UniffiCleaner {
    private val cleaner = android.system.SystemCleaner.cleaner()

    override fun register(resource: Any, disposable: Disposable): UniffiCleaner.Cleanable =
        AndroidSystemCleanable(cleaner.register(resource, UniffiCleanerAction(disposable)))
}

@RequiresApi(Build.VERSION_CODES.UPSIDE_DOWN_CAKE)
private class AndroidSystemCleanable(
    private val cleanable: java.lang.ref.Cleaner.Cleanable,
) : UniffiCleaner.Cleanable {
    override fun clean() = cleanable.clean()
}

private fun UniffiCleaner.Companion.create(): UniffiCleaner {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
        try {
            return AndroidSystemCleaner()
        } catch (_: IllegalAccessError) {
            // (For Compose preview) Fallback to UniffiJnaCleaner if AndroidSystemCleaner is
            // unavailable, even for API level 34 or higher.
        }
    }
    return UniffiJnaCleaner()
}


public object FfiConverterUShort: FfiConverter<UShort, Short> {
    override fun lift(value: Short): UShort {
        return value.toUShort()
    }

    override fun read(buf: ByteBuffer): UShort {
        return lift(buf.getShort())
    }

    override fun lower(value: UShort): Short {
        return value.toShort()
    }

    override fun allocationSize(value: UShort): ULong = 2UL

    override fun write(value: UShort, buf: ByteBuffer) {
        buf.putShort(value.toShort())
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


public object FfiConverterLong: FfiConverter<Long, Long> {
    override fun lift(value: Long): Long {
        return value
    }

    override fun read(buf: ByteBuffer): Long {
        return buf.getLong()
    }

    override fun lower(value: Long): Long {
        return value
    }

    override fun allocationSize(value: Long): ULong = 8UL

    override fun write(value: Long, buf: ByteBuffer) {
        buf.putLong(value)
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
public open class TrezorTransportCallbackImpl: Disposable, TrezorTransportCallback {

    public constructor(pointer: Pointer) {
        this.pointer = pointer
        this.cleanable = UniffiLib.CLEANER.register(this, UniffiPointerDestroyer(pointer))
    }

    /**
     * This constructor can be used to instantiate a fake object. Only used for tests. Any
     * attempt to actually use an object constructed this way will fail as there is no
     * connected Rust object.
     */
    public constructor(noPointer: NoPointer) {
        this.pointer = null
        this.cleanable = UniffiLib.CLEANER.register(this, UniffiPointerDestroyer(null))
    }

    protected val pointer: Pointer?
    protected val cleanable: UniffiCleaner.Cleanable

    private val wasDestroyed: kotlinx.atomicfu.AtomicBoolean = kotlinx.atomicfu.atomic(false)
    private val callCounter: kotlinx.atomicfu.AtomicLong = kotlinx.atomicfu.atomic(1L)

    private val lock = kotlinx.atomicfu.locks.ReentrantLock()

    private fun <T> synchronized(block: () -> T): T {
        lock.lock()
        try {
            return block()
        } finally {
            lock.unlock()
        }
    }

    override fun destroy() {
        // Only allow a single call to this method.
        // TODO: maybe we should log a warning if called more than once?
        if (this.wasDestroyed.compareAndSet(false, true)) {
            // This decrement always matches the initial count of 1 given at creation time.
            if (this.callCounter.decrementAndGet() == 0L) {
                cleanable.clean()
            }
        }
    }

    override fun close() {
        synchronized { this.destroy() }
    }

    internal inline fun <R> callWithPointer(block: (ptr: Pointer) -> R): R {
        // Check and increment the call counter, to keep the object alive.
        // This needs a compare-and-set retry loop in case of concurrent updates.
        do {
            val c = this.callCounter.value
            if (c == 0L) {
                throw IllegalStateException("${this::class::simpleName} object has already been destroyed")
            }
            if (c == Long.MAX_VALUE) {
                throw IllegalStateException("${this::class::simpleName} call counter would overflow")
            }
        } while (! this.callCounter.compareAndSet(c, c + 1L))
        // Now we can safely do the method call without the pointer being freed concurrently.
        try {
            return block(this.uniffiClonePointer())
        } finally {
            // This decrement always matches the increment we performed above.
            if (this.callCounter.decrementAndGet() == 0L) {
                cleanable.clean()
            }
        }
    }

    // Use a static inner class instead of a closure so as not to accidentally
    // capture `this` as part of the cleanable's action.
    private class UniffiPointerDestroyer(private val pointer: Pointer?) : Disposable {
        override fun destroy() {
            pointer?.let { ptr ->
                uniffiRustCall { status ->
                    UniffiLib.uniffi_bitkitcore_fn_free_trezortransportcallback(ptr, status)
                }
            }
        }
    }

    public fun uniffiClonePointer(): Pointer {
        return uniffiRustCall { status ->
            UniffiLib.uniffi_bitkitcore_fn_clone_trezortransportcallback(pointer!!, status)
        }!!
    }

    
    /**
     * Enumerate all connected Trezor devices
     */
    public override fun `enumerateDevices`(): List<NativeDeviceInfo> {
        return FfiConverterSequenceTypeNativeDeviceInfo.lift(callWithPointer {
            uniffiRustCall { uniffiRustCallStatus ->
                UniffiLib.uniffi_bitkitcore_fn_method_trezortransportcallback_enumerate_devices(
                    it,
                    uniffiRustCallStatus,
                )
            }
        })
    }

    /**
     * Open a connection to a device
     */
    public override fun `openDevice`(`path`: kotlin.String): TrezorTransportWriteResult {
        return FfiConverterTypeTrezorTransportWriteResult.lift(callWithPointer {
            uniffiRustCall { uniffiRustCallStatus ->
                UniffiLib.uniffi_bitkitcore_fn_method_trezortransportcallback_open_device(
                    it,
                    FfiConverterString.lower(`path`),
                    uniffiRustCallStatus,
                )
            }
        })
    }

    /**
     * Close the connection to a device
     */
    public override fun `closeDevice`(`path`: kotlin.String): TrezorTransportWriteResult {
        return FfiConverterTypeTrezorTransportWriteResult.lift(callWithPointer {
            uniffiRustCall { uniffiRustCallStatus ->
                UniffiLib.uniffi_bitkitcore_fn_method_trezortransportcallback_close_device(
                    it,
                    FfiConverterString.lower(`path`),
                    uniffiRustCallStatus,
                )
            }
        })
    }

    /**
     * Read a chunk of data from the device
     */
    public override fun `readChunk`(`path`: kotlin.String): TrezorTransportReadResult {
        return FfiConverterTypeTrezorTransportReadResult.lift(callWithPointer {
            uniffiRustCall { uniffiRustCallStatus ->
                UniffiLib.uniffi_bitkitcore_fn_method_trezortransportcallback_read_chunk(
                    it,
                    FfiConverterString.lower(`path`),
                    uniffiRustCallStatus,
                )
            }
        })
    }

    /**
     * Write a chunk of data to the device
     */
    public override fun `writeChunk`(`path`: kotlin.String, `data`: kotlin.ByteArray): TrezorTransportWriteResult {
        return FfiConverterTypeTrezorTransportWriteResult.lift(callWithPointer {
            uniffiRustCall { uniffiRustCallStatus ->
                UniffiLib.uniffi_bitkitcore_fn_method_trezortransportcallback_write_chunk(
                    it,
                    FfiConverterString.lower(`path`),
                    FfiConverterByteArray.lower(`data`),
                    uniffiRustCallStatus,
                )
            }
        })
    }

    /**
     * Get the chunk size for a device (64 for USB, 244 for Bluetooth)
     */
    public override fun `getChunkSize`(`path`: kotlin.String): kotlin.UInt {
        return FfiConverterUInt.lift(callWithPointer {
            uniffiRustCall { uniffiRustCallStatus ->
                UniffiLib.uniffi_bitkitcore_fn_method_trezortransportcallback_get_chunk_size(
                    it,
                    FfiConverterString.lower(`path`),
                    uniffiRustCallStatus,
                )
            }
        })
    }

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
    public override fun `callMessage`(`path`: kotlin.String, `messageType`: kotlin.UShort, `data`: kotlin.ByteArray): TrezorCallMessageResult? {
        return FfiConverterOptionalTypeTrezorCallMessageResult.lift(callWithPointer {
            uniffiRustCall { uniffiRustCallStatus ->
                UniffiLib.uniffi_bitkitcore_fn_method_trezortransportcallback_call_message(
                    it,
                    FfiConverterString.lower(`path`),
                    FfiConverterUShort.lower(`messageType`),
                    FfiConverterByteArray.lower(`data`),
                    uniffiRustCallStatus,
                )
            }
        })
    }

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
    public override fun `getPairingCode`(): kotlin.String {
        return FfiConverterString.lift(callWithPointer {
            uniffiRustCall { uniffiRustCallStatus ->
                UniffiLib.uniffi_bitkitcore_fn_method_trezortransportcallback_get_pairing_code(
                    it,
                    uniffiRustCallStatus,
                )
            }
        })
    }

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
    public override fun `saveThpCredential`(`deviceId`: kotlin.String, `credentialJson`: kotlin.String): kotlin.Boolean {
        return FfiConverterBoolean.lift(callWithPointer {
            uniffiRustCall { uniffiRustCallStatus ->
                UniffiLib.uniffi_bitkitcore_fn_method_trezortransportcallback_save_thp_credential(
                    it,
                    FfiConverterString.lower(`deviceId`),
                    FfiConverterString.lower(`credentialJson`),
                    uniffiRustCallStatus,
                )
            }
        })
    }

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
    public override fun `loadThpCredential`(`deviceId`: kotlin.String): kotlin.String? {
        return FfiConverterOptionalString.lift(callWithPointer {
            uniffiRustCall { uniffiRustCallStatus ->
                UniffiLib.uniffi_bitkitcore_fn_method_trezortransportcallback_load_thp_credential(
                    it,
                    FfiConverterString.lower(`deviceId`),
                    uniffiRustCallStatus,
                )
            }
        })
    }

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
    public override fun `logDebug`(`tag`: kotlin.String, `message`: kotlin.String) {
        callWithPointer {
            uniffiRustCall { uniffiRustCallStatus ->
                UniffiLib.uniffi_bitkitcore_fn_method_trezortransportcallback_log_debug(
                    it,
                    FfiConverterString.lower(`tag`),
                    FfiConverterString.lower(`message`),
                    uniffiRustCallStatus,
                )
            }
        }
    }


    
    

    
    
    public companion object
    
}





public object FfiConverterTypeTrezorTransportCallback: FfiConverter<TrezorTransportCallback, Pointer> {
    internal val handleMap = UniffiHandleMap<TrezorTransportCallback>()

    override fun lower(value: TrezorTransportCallback): Pointer {
        return handleMap.insert(value).toPointer()
    }

    override fun lift(value: Pointer): TrezorTransportCallback {
        return TrezorTransportCallbackImpl(value)
    }

    override fun read(buf: ByteBuffer): TrezorTransportCallback {
        // The Rust code always writes pointers as 8 bytes, and will
        // fail to compile if they don't fit.
        return lift(buf.getLong().toPointer())
    }

    override fun allocationSize(value: TrezorTransportCallback): ULong = 8UL

    override fun write(value: TrezorTransportCallback, buf: ByteBuffer) {
        // The Rust code always expects pointers written as 8 bytes,
        // and will fail to compile if they don't fit.
        buf.putLong(lower(value).toLong())
    }
}


// Put the implementation in an object so we don't pollute the top-level namespace
internal object uniffiCallbackInterfaceTrezorTransportCallback {
    internal object `enumerateDevices`: UniffiCallbackInterfaceTrezorTransportCallbackMethod0 {
        override fun callback (
            `uniffiHandle`: Long,
            `uniffiOutReturn`: RustBuffer,
            uniffiCallStatus: UniffiRustCallStatus,
        ) {
            val uniffiObj = FfiConverterTypeTrezorTransportCallback.handleMap.get(uniffiHandle)
            val makeCall = { ->
                uniffiObj.`enumerateDevices`(
                )
            }
            val writeReturn = { uniffiResultValue: List<NativeDeviceInfo> ->
                uniffiOutReturn.setValue(FfiConverterSequenceTypeNativeDeviceInfo.lower(uniffiResultValue))
            }
            uniffiTraitInterfaceCall(uniffiCallStatus, makeCall, writeReturn)
        }
    }
    internal object `openDevice`: UniffiCallbackInterfaceTrezorTransportCallbackMethod1 {
        override fun callback (
            `uniffiHandle`: Long,
            `path`: RustBufferByValue,
            `uniffiOutReturn`: RustBuffer,
            uniffiCallStatus: UniffiRustCallStatus,
        ) {
            val uniffiObj = FfiConverterTypeTrezorTransportCallback.handleMap.get(uniffiHandle)
            val makeCall = { ->
                uniffiObj.`openDevice`(
                    FfiConverterString.lift(`path`),
                )
            }
            val writeReturn = { uniffiResultValue: TrezorTransportWriteResult ->
                uniffiOutReturn.setValue(FfiConverterTypeTrezorTransportWriteResult.lower(uniffiResultValue))
            }
            uniffiTraitInterfaceCall(uniffiCallStatus, makeCall, writeReturn)
        }
    }
    internal object `closeDevice`: UniffiCallbackInterfaceTrezorTransportCallbackMethod2 {
        override fun callback (
            `uniffiHandle`: Long,
            `path`: RustBufferByValue,
            `uniffiOutReturn`: RustBuffer,
            uniffiCallStatus: UniffiRustCallStatus,
        ) {
            val uniffiObj = FfiConverterTypeTrezorTransportCallback.handleMap.get(uniffiHandle)
            val makeCall = { ->
                uniffiObj.`closeDevice`(
                    FfiConverterString.lift(`path`),
                )
            }
            val writeReturn = { uniffiResultValue: TrezorTransportWriteResult ->
                uniffiOutReturn.setValue(FfiConverterTypeTrezorTransportWriteResult.lower(uniffiResultValue))
            }
            uniffiTraitInterfaceCall(uniffiCallStatus, makeCall, writeReturn)
        }
    }
    internal object `readChunk`: UniffiCallbackInterfaceTrezorTransportCallbackMethod3 {
        override fun callback (
            `uniffiHandle`: Long,
            `path`: RustBufferByValue,
            `uniffiOutReturn`: RustBuffer,
            uniffiCallStatus: UniffiRustCallStatus,
        ) {
            val uniffiObj = FfiConverterTypeTrezorTransportCallback.handleMap.get(uniffiHandle)
            val makeCall = { ->
                uniffiObj.`readChunk`(
                    FfiConverterString.lift(`path`),
                )
            }
            val writeReturn = { uniffiResultValue: TrezorTransportReadResult ->
                uniffiOutReturn.setValue(FfiConverterTypeTrezorTransportReadResult.lower(uniffiResultValue))
            }
            uniffiTraitInterfaceCall(uniffiCallStatus, makeCall, writeReturn)
        }
    }
    internal object `writeChunk`: UniffiCallbackInterfaceTrezorTransportCallbackMethod4 {
        override fun callback (
            `uniffiHandle`: Long,
            `path`: RustBufferByValue,
            `data`: RustBufferByValue,
            `uniffiOutReturn`: RustBuffer,
            uniffiCallStatus: UniffiRustCallStatus,
        ) {
            val uniffiObj = FfiConverterTypeTrezorTransportCallback.handleMap.get(uniffiHandle)
            val makeCall = { ->
                uniffiObj.`writeChunk`(
                    FfiConverterString.lift(`path`),
                    FfiConverterByteArray.lift(`data`),
                )
            }
            val writeReturn = { uniffiResultValue: TrezorTransportWriteResult ->
                uniffiOutReturn.setValue(FfiConverterTypeTrezorTransportWriteResult.lower(uniffiResultValue))
            }
            uniffiTraitInterfaceCall(uniffiCallStatus, makeCall, writeReturn)
        }
    }
    internal object `getChunkSize`: UniffiCallbackInterfaceTrezorTransportCallbackMethod5 {
        override fun callback (
            `uniffiHandle`: Long,
            `path`: RustBufferByValue,
            `uniffiOutReturn`: IntByReference,
            uniffiCallStatus: UniffiRustCallStatus,
        ) {
            val uniffiObj = FfiConverterTypeTrezorTransportCallback.handleMap.get(uniffiHandle)
            val makeCall = { ->
                uniffiObj.`getChunkSize`(
                    FfiConverterString.lift(`path`),
                )
            }
            val writeReturn = { uniffiResultValue: kotlin.UInt ->
                uniffiOutReturn.setValue(FfiConverterUInt.lower(uniffiResultValue))
            }
            uniffiTraitInterfaceCall(uniffiCallStatus, makeCall, writeReturn)
        }
    }
    internal object `callMessage`: UniffiCallbackInterfaceTrezorTransportCallbackMethod6 {
        override fun callback (
            `uniffiHandle`: Long,
            `path`: RustBufferByValue,
            `messageType`: Short,
            `data`: RustBufferByValue,
            `uniffiOutReturn`: RustBuffer,
            uniffiCallStatus: UniffiRustCallStatus,
        ) {
            val uniffiObj = FfiConverterTypeTrezorTransportCallback.handleMap.get(uniffiHandle)
            val makeCall = { ->
                uniffiObj.`callMessage`(
                    FfiConverterString.lift(`path`),
                    FfiConverterUShort.lift(`messageType`),
                    FfiConverterByteArray.lift(`data`),
                )
            }
            val writeReturn = { uniffiResultValue: TrezorCallMessageResult? ->
                uniffiOutReturn.setValue(FfiConverterOptionalTypeTrezorCallMessageResult.lower(uniffiResultValue))
            }
            uniffiTraitInterfaceCall(uniffiCallStatus, makeCall, writeReturn)
        }
    }
    internal object `getPairingCode`: UniffiCallbackInterfaceTrezorTransportCallbackMethod7 {
        override fun callback (
            `uniffiHandle`: Long,
            `uniffiOutReturn`: RustBuffer,
            uniffiCallStatus: UniffiRustCallStatus,
        ) {
            val uniffiObj = FfiConverterTypeTrezorTransportCallback.handleMap.get(uniffiHandle)
            val makeCall = { ->
                uniffiObj.`getPairingCode`(
                )
            }
            val writeReturn = { uniffiResultValue: kotlin.String ->
                uniffiOutReturn.setValue(FfiConverterString.lower(uniffiResultValue))
            }
            uniffiTraitInterfaceCall(uniffiCallStatus, makeCall, writeReturn)
        }
    }
    internal object `saveThpCredential`: UniffiCallbackInterfaceTrezorTransportCallbackMethod8 {
        override fun callback (
            `uniffiHandle`: Long,
            `deviceId`: RustBufferByValue,
            `credentialJson`: RustBufferByValue,
            `uniffiOutReturn`: ByteByReference,
            uniffiCallStatus: UniffiRustCallStatus,
        ) {
            val uniffiObj = FfiConverterTypeTrezorTransportCallback.handleMap.get(uniffiHandle)
            val makeCall = { ->
                uniffiObj.`saveThpCredential`(
                    FfiConverterString.lift(`deviceId`),
                    FfiConverterString.lift(`credentialJson`),
                )
            }
            val writeReturn = { uniffiResultValue: kotlin.Boolean ->
                uniffiOutReturn.setValue(FfiConverterBoolean.lower(uniffiResultValue))
            }
            uniffiTraitInterfaceCall(uniffiCallStatus, makeCall, writeReturn)
        }
    }
    internal object `loadThpCredential`: UniffiCallbackInterfaceTrezorTransportCallbackMethod9 {
        override fun callback (
            `uniffiHandle`: Long,
            `deviceId`: RustBufferByValue,
            `uniffiOutReturn`: RustBuffer,
            uniffiCallStatus: UniffiRustCallStatus,
        ) {
            val uniffiObj = FfiConverterTypeTrezorTransportCallback.handleMap.get(uniffiHandle)
            val makeCall = { ->
                uniffiObj.`loadThpCredential`(
                    FfiConverterString.lift(`deviceId`),
                )
            }
            val writeReturn = { uniffiResultValue: kotlin.String? ->
                uniffiOutReturn.setValue(FfiConverterOptionalString.lower(uniffiResultValue))
            }
            uniffiTraitInterfaceCall(uniffiCallStatus, makeCall, writeReturn)
        }
    }
    internal object `logDebug`: UniffiCallbackInterfaceTrezorTransportCallbackMethod10 {
        override fun callback (
            `uniffiHandle`: Long,
            `tag`: RustBufferByValue,
            `message`: RustBufferByValue,
            `uniffiOutReturn`: Pointer,
            uniffiCallStatus: UniffiRustCallStatus,
        ) {
            val uniffiObj = FfiConverterTypeTrezorTransportCallback.handleMap.get(uniffiHandle)
            val makeCall = { ->
                uniffiObj.`logDebug`(
                    FfiConverterString.lift(`tag`),
                    FfiConverterString.lift(`message`),
                )
            }
            val writeReturn = { _: Unit ->
                @Suppress("UNUSED_EXPRESSION")
                uniffiOutReturn
                Unit
            }
            uniffiTraitInterfaceCall(uniffiCallStatus, makeCall, writeReturn)
        }
    }
    internal object uniffiFree: UniffiCallbackInterfaceFree {
        override fun callback(handle: Long) {
            FfiConverterTypeTrezorTransportCallback.handleMap.remove(handle)
        }
    }

    internal val vtable = UniffiVTableCallbackInterfaceTrezorTransportCallback(
        `enumerateDevices`,
        `openDevice`,
        `closeDevice`,
        `readChunk`,
        `writeChunk`,
        `getChunkSize`,
        `callMessage`,
        `getPairingCode`,
        `saveThpCredential`,
        `loadThpCredential`,
        `logDebug`,
        uniffiFree,
    )

    internal fun register(lib: UniffiLib) {
        lib.uniffi_bitkitcore_fn_init_callback_vtable_trezortransportcallback(vtable)
    }
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
public open class TrezorUiCallbackImpl: Disposable, TrezorUiCallback {

    public constructor(pointer: Pointer) {
        this.pointer = pointer
        this.cleanable = UniffiLib.CLEANER.register(this, UniffiPointerDestroyer(pointer))
    }

    /**
     * This constructor can be used to instantiate a fake object. Only used for tests. Any
     * attempt to actually use an object constructed this way will fail as there is no
     * connected Rust object.
     */
    public constructor(noPointer: NoPointer) {
        this.pointer = null
        this.cleanable = UniffiLib.CLEANER.register(this, UniffiPointerDestroyer(null))
    }

    protected val pointer: Pointer?
    protected val cleanable: UniffiCleaner.Cleanable

    private val wasDestroyed: kotlinx.atomicfu.AtomicBoolean = kotlinx.atomicfu.atomic(false)
    private val callCounter: kotlinx.atomicfu.AtomicLong = kotlinx.atomicfu.atomic(1L)

    private val lock = kotlinx.atomicfu.locks.ReentrantLock()

    private fun <T> synchronized(block: () -> T): T {
        lock.lock()
        try {
            return block()
        } finally {
            lock.unlock()
        }
    }

    override fun destroy() {
        // Only allow a single call to this method.
        // TODO: maybe we should log a warning if called more than once?
        if (this.wasDestroyed.compareAndSet(false, true)) {
            // This decrement always matches the initial count of 1 given at creation time.
            if (this.callCounter.decrementAndGet() == 0L) {
                cleanable.clean()
            }
        }
    }

    override fun close() {
        synchronized { this.destroy() }
    }

    internal inline fun <R> callWithPointer(block: (ptr: Pointer) -> R): R {
        // Check and increment the call counter, to keep the object alive.
        // This needs a compare-and-set retry loop in case of concurrent updates.
        do {
            val c = this.callCounter.value
            if (c == 0L) {
                throw IllegalStateException("${this::class::simpleName} object has already been destroyed")
            }
            if (c == Long.MAX_VALUE) {
                throw IllegalStateException("${this::class::simpleName} call counter would overflow")
            }
        } while (! this.callCounter.compareAndSet(c, c + 1L))
        // Now we can safely do the method call without the pointer being freed concurrently.
        try {
            return block(this.uniffiClonePointer())
        } finally {
            // This decrement always matches the increment we performed above.
            if (this.callCounter.decrementAndGet() == 0L) {
                cleanable.clean()
            }
        }
    }

    // Use a static inner class instead of a closure so as not to accidentally
    // capture `this` as part of the cleanable's action.
    private class UniffiPointerDestroyer(private val pointer: Pointer?) : Disposable {
        override fun destroy() {
            pointer?.let { ptr ->
                uniffiRustCall { status ->
                    UniffiLib.uniffi_bitkitcore_fn_free_trezoruicallback(ptr, status)
                }
            }
        }
    }

    public fun uniffiClonePointer(): Pointer {
        return uniffiRustCall { status ->
            UniffiLib.uniffi_bitkitcore_fn_clone_trezoruicallback(pointer!!, status)
        }!!
    }

    
    /**
     * Called when the device requests a PIN.
     *
     * Show a PIN matrix UI and return the matrix-encoded PIN string.
     * Return empty string to cancel.
     */
    public override fun `onPinRequest`(): kotlin.String {
        return FfiConverterString.lift(callWithPointer {
            uniffiRustCall { uniffiRustCallStatus ->
                UniffiLib.uniffi_bitkitcore_fn_method_trezoruicallback_on_pin_request(
                    it,
                    uniffiRustCallStatus,
                )
            }
        })
    }

    /**
     * Called when the device requests a passphrase.
     *
     * If `on_device` is true, the user should enter on the Trezor itself —
     * return any non-empty string (e.g., "ok") to acknowledge.
     *
     * If `on_device` is false, show a passphrase input UI and return the value.
     * Return empty string to cancel.
     */
    public override fun `onPassphraseRequest`(`onDevice`: kotlin.Boolean): kotlin.String {
        return FfiConverterString.lift(callWithPointer {
            uniffiRustCall { uniffiRustCallStatus ->
                UniffiLib.uniffi_bitkitcore_fn_method_trezoruicallback_on_passphrase_request(
                    it,
                    FfiConverterBoolean.lower(`onDevice`),
                    uniffiRustCallStatus,
                )
            }
        })
    }


    
    

    
    
    public companion object
    
}





public object FfiConverterTypeTrezorUiCallback: FfiConverter<TrezorUiCallback, Pointer> {
    internal val handleMap = UniffiHandleMap<TrezorUiCallback>()

    override fun lower(value: TrezorUiCallback): Pointer {
        return handleMap.insert(value).toPointer()
    }

    override fun lift(value: Pointer): TrezorUiCallback {
        return TrezorUiCallbackImpl(value)
    }

    override fun read(buf: ByteBuffer): TrezorUiCallback {
        // The Rust code always writes pointers as 8 bytes, and will
        // fail to compile if they don't fit.
        return lift(buf.getLong().toPointer())
    }

    override fun allocationSize(value: TrezorUiCallback): ULong = 8UL

    override fun write(value: TrezorUiCallback, buf: ByteBuffer) {
        // The Rust code always expects pointers written as 8 bytes,
        // and will fail to compile if they don't fit.
        buf.putLong(lower(value).toLong())
    }
}


// Put the implementation in an object so we don't pollute the top-level namespace
internal object uniffiCallbackInterfaceTrezorUiCallback {
    internal object `onPinRequest`: UniffiCallbackInterfaceTrezorUiCallbackMethod0 {
        override fun callback (
            `uniffiHandle`: Long,
            `uniffiOutReturn`: RustBuffer,
            uniffiCallStatus: UniffiRustCallStatus,
        ) {
            val uniffiObj = FfiConverterTypeTrezorUiCallback.handleMap.get(uniffiHandle)
            val makeCall = { ->
                uniffiObj.`onPinRequest`(
                )
            }
            val writeReturn = { uniffiResultValue: kotlin.String ->
                uniffiOutReturn.setValue(FfiConverterString.lower(uniffiResultValue))
            }
            uniffiTraitInterfaceCall(uniffiCallStatus, makeCall, writeReturn)
        }
    }
    internal object `onPassphraseRequest`: UniffiCallbackInterfaceTrezorUiCallbackMethod1 {
        override fun callback (
            `uniffiHandle`: Long,
            `onDevice`: Byte,
            `uniffiOutReturn`: RustBuffer,
            uniffiCallStatus: UniffiRustCallStatus,
        ) {
            val uniffiObj = FfiConverterTypeTrezorUiCallback.handleMap.get(uniffiHandle)
            val makeCall = { ->
                uniffiObj.`onPassphraseRequest`(
                    FfiConverterBoolean.lift(`onDevice`),
                )
            }
            val writeReturn = { uniffiResultValue: kotlin.String ->
                uniffiOutReturn.setValue(FfiConverterString.lower(uniffiResultValue))
            }
            uniffiTraitInterfaceCall(uniffiCallStatus, makeCall, writeReturn)
        }
    }
    internal object uniffiFree: UniffiCallbackInterfaceFree {
        override fun callback(handle: Long) {
            FfiConverterTypeTrezorUiCallback.handleMap.remove(handle)
        }
    }

    internal val vtable = UniffiVTableCallbackInterfaceTrezorUiCallback(
        `onPinRequest`,
        `onPassphraseRequest`,
        uniffiFree,
    )

    internal fun register(lib: UniffiLib) {
        lib.uniffi_bitkitcore_fn_init_callback_vtable_trezoruicallback(vtable)
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




public object FfiConverterTypeChannelLiquidityOptions: FfiConverterRustBuffer<ChannelLiquidityOptions> {
    override fun read(buf: ByteBuffer): ChannelLiquidityOptions {
        return ChannelLiquidityOptions(
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
        )
    }

    override fun allocationSize(value: ChannelLiquidityOptions): ULong = (
            FfiConverterULong.allocationSize(value.`defaultLspBalanceSat`) +
            FfiConverterULong.allocationSize(value.`minLspBalanceSat`) +
            FfiConverterULong.allocationSize(value.`maxLspBalanceSat`) +
            FfiConverterULong.allocationSize(value.`maxClientBalanceSat`)
    )

    override fun write(value: ChannelLiquidityOptions, buf: ByteBuffer) {
        FfiConverterULong.write(value.`defaultLspBalanceSat`, buf)
        FfiConverterULong.write(value.`minLspBalanceSat`, buf)
        FfiConverterULong.write(value.`maxLspBalanceSat`, buf)
        FfiConverterULong.write(value.`maxClientBalanceSat`, buf)
    }
}




public object FfiConverterTypeChannelLiquidityParams: FfiConverterRustBuffer<ChannelLiquidityParams> {
    override fun read(buf: ByteBuffer): ChannelLiquidityParams {
        return ChannelLiquidityParams(
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
        )
    }

    override fun allocationSize(value: ChannelLiquidityParams): ULong = (
            FfiConverterULong.allocationSize(value.`clientBalanceSat`) +
            FfiConverterULong.allocationSize(value.`existingChannelsTotalSat`) +
            FfiConverterULong.allocationSize(value.`minChannelSizeSat`) +
            FfiConverterULong.allocationSize(value.`maxChannelSizeSat`) +
            FfiConverterULong.allocationSize(value.`satsPerEur`)
    )

    override fun write(value: ChannelLiquidityParams, buf: ByteBuffer) {
        FfiConverterULong.write(value.`clientBalanceSat`, buf)
        FfiConverterULong.write(value.`existingChannelsTotalSat`, buf)
        FfiConverterULong.write(value.`minChannelSizeSat`, buf)
        FfiConverterULong.write(value.`maxChannelSizeSat`, buf)
        FfiConverterULong.write(value.`satsPerEur`, buf)
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




public object FfiConverterTypeDefaultLspBalanceParams: FfiConverterRustBuffer<DefaultLspBalanceParams> {
    override fun read(buf: ByteBuffer): DefaultLspBalanceParams {
        return DefaultLspBalanceParams(
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
        )
    }

    override fun allocationSize(value: DefaultLspBalanceParams): ULong = (
            FfiConverterULong.allocationSize(value.`clientBalanceSat`) +
            FfiConverterULong.allocationSize(value.`maxChannelSizeSat`) +
            FfiConverterULong.allocationSize(value.`satsPerEur`)
    )

    override fun write(value: DefaultLspBalanceParams, buf: ByteBuffer) {
        FfiConverterULong.write(value.`clientBalanceSat`, buf)
        FfiConverterULong.write(value.`maxChannelSizeSat`, buf)
        FfiConverterULong.write(value.`satsPerEur`, buf)
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
            FfiConverterOptionalULong.allocationSize(value.`updatedAt`) +
            FfiConverterOptionalULong.allocationSize(value.`seenAt`)
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
        FfiConverterOptionalULong.write(value.`seenAt`, buf)
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




public object FfiConverterTypeNativeDeviceInfo: FfiConverterRustBuffer<NativeDeviceInfo> {
    override fun read(buf: ByteBuffer): NativeDeviceInfo {
        return NativeDeviceInfo(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalUShort.read(buf),
            FfiConverterOptionalUShort.read(buf),
        )
    }

    override fun allocationSize(value: NativeDeviceInfo): ULong = (
            FfiConverterString.allocationSize(value.`path`) +
            FfiConverterString.allocationSize(value.`transportType`) +
            FfiConverterOptionalString.allocationSize(value.`name`) +
            FfiConverterOptionalUShort.allocationSize(value.`vendorId`) +
            FfiConverterOptionalUShort.allocationSize(value.`productId`)
    )

    override fun write(value: NativeDeviceInfo, buf: ByteBuffer) {
        FfiConverterString.write(value.`path`, buf)
        FfiConverterString.write(value.`transportType`, buf)
        FfiConverterOptionalString.write(value.`name`, buf)
        FfiConverterOptionalUShort.write(value.`vendorId`, buf)
        FfiConverterOptionalUShort.write(value.`productId`, buf)
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
            FfiConverterOptionalULong.allocationSize(value.`updatedAt`) +
            FfiConverterOptionalULong.allocationSize(value.`seenAt`)
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
        FfiConverterOptionalULong.write(value.`seenAt`, buf)
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




public object FfiConverterTypeSweepResult: FfiConverterRustBuffer<SweepResult> {
    override fun read(buf: ByteBuffer): SweepResult {
        return SweepResult(
            FfiConverterString.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterUInt.read(buf),
        )
    }

    override fun allocationSize(value: SweepResult): ULong = (
            FfiConverterString.allocationSize(value.`txid`) +
            FfiConverterULong.allocationSize(value.`amountSwept`) +
            FfiConverterULong.allocationSize(value.`feePaid`) +
            FfiConverterUInt.allocationSize(value.`utxosSwept`)
    )

    override fun write(value: SweepResult, buf: ByteBuffer) {
        FfiConverterString.write(value.`txid`, buf)
        FfiConverterULong.write(value.`amountSwept`, buf)
        FfiConverterULong.write(value.`feePaid`, buf)
        FfiConverterUInt.write(value.`utxosSwept`, buf)
    }
}




public object FfiConverterTypeSweepTransactionPreview: FfiConverterRustBuffer<SweepTransactionPreview> {
    override fun read(buf: ByteBuffer): SweepTransactionPreview {
        return SweepTransactionPreview(
            FfiConverterString.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterULong.read(buf),
        )
    }

    override fun allocationSize(value: SweepTransactionPreview): ULong = (
            FfiConverterString.allocationSize(value.`psbt`) +
            FfiConverterULong.allocationSize(value.`totalAmount`) +
            FfiConverterULong.allocationSize(value.`estimatedFee`) +
            FfiConverterULong.allocationSize(value.`estimatedVsize`) +
            FfiConverterUInt.allocationSize(value.`utxosCount`) +
            FfiConverterString.allocationSize(value.`destinationAddress`) +
            FfiConverterULong.allocationSize(value.`amountAfterFees`)
    )

    override fun write(value: SweepTransactionPreview, buf: ByteBuffer) {
        FfiConverterString.write(value.`psbt`, buf)
        FfiConverterULong.write(value.`totalAmount`, buf)
        FfiConverterULong.write(value.`estimatedFee`, buf)
        FfiConverterULong.write(value.`estimatedVsize`, buf)
        FfiConverterUInt.write(value.`utxosCount`, buf)
        FfiConverterString.write(value.`destinationAddress`, buf)
        FfiConverterULong.write(value.`amountAfterFees`, buf)
    }
}




public object FfiConverterTypeSweepableBalances: FfiConverterRustBuffer<SweepableBalances> {
    override fun read(buf: ByteBuffer): SweepableBalances {
        return SweepableBalances(
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
        )
    }

    override fun allocationSize(value: SweepableBalances): ULong = (
            FfiConverterULong.allocationSize(value.`legacyBalance`) +
            FfiConverterULong.allocationSize(value.`p2shBalance`) +
            FfiConverterULong.allocationSize(value.`taprootBalance`) +
            FfiConverterULong.allocationSize(value.`totalBalance`) +
            FfiConverterUInt.allocationSize(value.`legacyUtxosCount`) +
            FfiConverterUInt.allocationSize(value.`p2shUtxosCount`) +
            FfiConverterUInt.allocationSize(value.`taprootUtxosCount`) +
            FfiConverterUInt.allocationSize(value.`totalUtxosCount`)
    )

    override fun write(value: SweepableBalances, buf: ByteBuffer) {
        FfiConverterULong.write(value.`legacyBalance`, buf)
        FfiConverterULong.write(value.`p2shBalance`, buf)
        FfiConverterULong.write(value.`taprootBalance`, buf)
        FfiConverterULong.write(value.`totalBalance`, buf)
        FfiConverterUInt.write(value.`legacyUtxosCount`, buf)
        FfiConverterUInt.write(value.`p2shUtxosCount`, buf)
        FfiConverterUInt.write(value.`taprootUtxosCount`, buf)
        FfiConverterUInt.write(value.`totalUtxosCount`, buf)
    }
}




public object FfiConverterTypeTransactionDetails: FfiConverterRustBuffer<TransactionDetails> {
    override fun read(buf: ByteBuffer): TransactionDetails {
        return TransactionDetails(
            FfiConverterString.read(buf),
            FfiConverterLong.read(buf),
            FfiConverterSequenceTypeTxInput.read(buf),
            FfiConverterSequenceTypeTxOutput.read(buf),
        )
    }

    override fun allocationSize(value: TransactionDetails): ULong = (
            FfiConverterString.allocationSize(value.`txId`) +
            FfiConverterLong.allocationSize(value.`amountSats`) +
            FfiConverterSequenceTypeTxInput.allocationSize(value.`inputs`) +
            FfiConverterSequenceTypeTxOutput.allocationSize(value.`outputs`)
    )

    override fun write(value: TransactionDetails, buf: ByteBuffer) {
        FfiConverterString.write(value.`txId`, buf)
        FfiConverterLong.write(value.`amountSats`, buf)
        FfiConverterSequenceTypeTxInput.write(value.`inputs`, buf)
        FfiConverterSequenceTypeTxOutput.write(value.`outputs`, buf)
    }
}




public object FfiConverterTypeTrezorAddressResponse: FfiConverterRustBuffer<TrezorAddressResponse> {
    override fun read(buf: ByteBuffer): TrezorAddressResponse {
        return TrezorAddressResponse(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: TrezorAddressResponse): ULong = (
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterString.allocationSize(value.`path`)
    )

    override fun write(value: TrezorAddressResponse, buf: ByteBuffer) {
        FfiConverterString.write(value.`address`, buf)
        FfiConverterString.write(value.`path`, buf)
    }
}




public object FfiConverterTypeTrezorCallMessageResult: FfiConverterRustBuffer<TrezorCallMessageResult> {
    override fun read(buf: ByteBuffer): TrezorCallMessageResult {
        return TrezorCallMessageResult(
            FfiConverterBoolean.read(buf),
            FfiConverterUShort.read(buf),
            FfiConverterByteArray.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: TrezorCallMessageResult): ULong = (
            FfiConverterBoolean.allocationSize(value.`success`) +
            FfiConverterUShort.allocationSize(value.`messageType`) +
            FfiConverterByteArray.allocationSize(value.`data`) +
            FfiConverterString.allocationSize(value.`error`)
    )

    override fun write(value: TrezorCallMessageResult, buf: ByteBuffer) {
        FfiConverterBoolean.write(value.`success`, buf)
        FfiConverterUShort.write(value.`messageType`, buf)
        FfiConverterByteArray.write(value.`data`, buf)
        FfiConverterString.write(value.`error`, buf)
    }
}




public object FfiConverterTypeTrezorDeviceInfo: FfiConverterRustBuffer<TrezorDeviceInfo> {
    override fun read(buf: ByteBuffer): TrezorDeviceInfo {
        return TrezorDeviceInfo(
            FfiConverterString.read(buf),
            FfiConverterTypeTrezorTransportType.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterBoolean.read(buf),
        )
    }

    override fun allocationSize(value: TrezorDeviceInfo): ULong = (
            FfiConverterString.allocationSize(value.`id`) +
            FfiConverterTypeTrezorTransportType.allocationSize(value.`transportType`) +
            FfiConverterOptionalString.allocationSize(value.`name`) +
            FfiConverterString.allocationSize(value.`path`) +
            FfiConverterOptionalString.allocationSize(value.`label`) +
            FfiConverterOptionalString.allocationSize(value.`model`) +
            FfiConverterBoolean.allocationSize(value.`isBootloader`)
    )

    override fun write(value: TrezorDeviceInfo, buf: ByteBuffer) {
        FfiConverterString.write(value.`id`, buf)
        FfiConverterTypeTrezorTransportType.write(value.`transportType`, buf)
        FfiConverterOptionalString.write(value.`name`, buf)
        FfiConverterString.write(value.`path`, buf)
        FfiConverterOptionalString.write(value.`label`, buf)
        FfiConverterOptionalString.write(value.`model`, buf)
        FfiConverterBoolean.write(value.`isBootloader`, buf)
    }
}




public object FfiConverterTypeTrezorFeatures: FfiConverterRustBuffer<TrezorFeatures> {
    override fun read(buf: ByteBuffer): TrezorFeatures {
        return TrezorFeatures(
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalBoolean.read(buf),
            FfiConverterOptionalBoolean.read(buf),
        )
    }

    override fun allocationSize(value: TrezorFeatures): ULong = (
            FfiConverterOptionalString.allocationSize(value.`vendor`) +
            FfiConverterOptionalString.allocationSize(value.`model`) +
            FfiConverterOptionalString.allocationSize(value.`label`) +
            FfiConverterOptionalString.allocationSize(value.`deviceId`) +
            FfiConverterOptionalUInt.allocationSize(value.`majorVersion`) +
            FfiConverterOptionalUInt.allocationSize(value.`minorVersion`) +
            FfiConverterOptionalUInt.allocationSize(value.`patchVersion`) +
            FfiConverterOptionalBoolean.allocationSize(value.`pinProtection`) +
            FfiConverterOptionalBoolean.allocationSize(value.`passphraseProtection`) +
            FfiConverterOptionalBoolean.allocationSize(value.`initialized`) +
            FfiConverterOptionalBoolean.allocationSize(value.`needsBackup`)
    )

    override fun write(value: TrezorFeatures, buf: ByteBuffer) {
        FfiConverterOptionalString.write(value.`vendor`, buf)
        FfiConverterOptionalString.write(value.`model`, buf)
        FfiConverterOptionalString.write(value.`label`, buf)
        FfiConverterOptionalString.write(value.`deviceId`, buf)
        FfiConverterOptionalUInt.write(value.`majorVersion`, buf)
        FfiConverterOptionalUInt.write(value.`minorVersion`, buf)
        FfiConverterOptionalUInt.write(value.`patchVersion`, buf)
        FfiConverterOptionalBoolean.write(value.`pinProtection`, buf)
        FfiConverterOptionalBoolean.write(value.`passphraseProtection`, buf)
        FfiConverterOptionalBoolean.write(value.`initialized`, buf)
        FfiConverterOptionalBoolean.write(value.`needsBackup`, buf)
    }
}




public object FfiConverterTypeTrezorGetAddressParams: FfiConverterRustBuffer<TrezorGetAddressParams> {
    override fun read(buf: ByteBuffer): TrezorGetAddressParams {
        return TrezorGetAddressParams(
            FfiConverterString.read(buf),
            FfiConverterOptionalTypeTrezorCoinType.read(buf),
            FfiConverterBoolean.read(buf),
            FfiConverterOptionalTypeTrezorScriptType.read(buf),
        )
    }

    override fun allocationSize(value: TrezorGetAddressParams): ULong = (
            FfiConverterString.allocationSize(value.`path`) +
            FfiConverterOptionalTypeTrezorCoinType.allocationSize(value.`coin`) +
            FfiConverterBoolean.allocationSize(value.`showOnTrezor`) +
            FfiConverterOptionalTypeTrezorScriptType.allocationSize(value.`scriptType`)
    )

    override fun write(value: TrezorGetAddressParams, buf: ByteBuffer) {
        FfiConverterString.write(value.`path`, buf)
        FfiConverterOptionalTypeTrezorCoinType.write(value.`coin`, buf)
        FfiConverterBoolean.write(value.`showOnTrezor`, buf)
        FfiConverterOptionalTypeTrezorScriptType.write(value.`scriptType`, buf)
    }
}




public object FfiConverterTypeTrezorGetPublicKeyParams: FfiConverterRustBuffer<TrezorGetPublicKeyParams> {
    override fun read(buf: ByteBuffer): TrezorGetPublicKeyParams {
        return TrezorGetPublicKeyParams(
            FfiConverterString.read(buf),
            FfiConverterOptionalTypeTrezorCoinType.read(buf),
            FfiConverterBoolean.read(buf),
        )
    }

    override fun allocationSize(value: TrezorGetPublicKeyParams): ULong = (
            FfiConverterString.allocationSize(value.`path`) +
            FfiConverterOptionalTypeTrezorCoinType.allocationSize(value.`coin`) +
            FfiConverterBoolean.allocationSize(value.`showOnTrezor`)
    )

    override fun write(value: TrezorGetPublicKeyParams, buf: ByteBuffer) {
        FfiConverterString.write(value.`path`, buf)
        FfiConverterOptionalTypeTrezorCoinType.write(value.`coin`, buf)
        FfiConverterBoolean.write(value.`showOnTrezor`, buf)
    }
}




public object FfiConverterTypeTrezorPrevTx: FfiConverterRustBuffer<TrezorPrevTx> {
    override fun read(buf: ByteBuffer): TrezorPrevTx {
        return TrezorPrevTx(
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterSequenceTypeTrezorPrevTxInput.read(buf),
            FfiConverterSequenceTypeTrezorPrevTxOutput.read(buf),
        )
    }

    override fun allocationSize(value: TrezorPrevTx): ULong = (
            FfiConverterString.allocationSize(value.`hash`) +
            FfiConverterUInt.allocationSize(value.`version`) +
            FfiConverterUInt.allocationSize(value.`lockTime`) +
            FfiConverterSequenceTypeTrezorPrevTxInput.allocationSize(value.`inputs`) +
            FfiConverterSequenceTypeTrezorPrevTxOutput.allocationSize(value.`outputs`)
    )

    override fun write(value: TrezorPrevTx, buf: ByteBuffer) {
        FfiConverterString.write(value.`hash`, buf)
        FfiConverterUInt.write(value.`version`, buf)
        FfiConverterUInt.write(value.`lockTime`, buf)
        FfiConverterSequenceTypeTrezorPrevTxInput.write(value.`inputs`, buf)
        FfiConverterSequenceTypeTrezorPrevTxOutput.write(value.`outputs`, buf)
    }
}




public object FfiConverterTypeTrezorPrevTxInput: FfiConverterRustBuffer<TrezorPrevTxInput> {
    override fun read(buf: ByteBuffer): TrezorPrevTxInput {
        return TrezorPrevTxInput(
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
        )
    }

    override fun allocationSize(value: TrezorPrevTxInput): ULong = (
            FfiConverterString.allocationSize(value.`prevHash`) +
            FfiConverterUInt.allocationSize(value.`prevIndex`) +
            FfiConverterString.allocationSize(value.`scriptSig`) +
            FfiConverterUInt.allocationSize(value.`sequence`)
    )

    override fun write(value: TrezorPrevTxInput, buf: ByteBuffer) {
        FfiConverterString.write(value.`prevHash`, buf)
        FfiConverterUInt.write(value.`prevIndex`, buf)
        FfiConverterString.write(value.`scriptSig`, buf)
        FfiConverterUInt.write(value.`sequence`, buf)
    }
}




public object FfiConverterTypeTrezorPrevTxOutput: FfiConverterRustBuffer<TrezorPrevTxOutput> {
    override fun read(buf: ByteBuffer): TrezorPrevTxOutput {
        return TrezorPrevTxOutput(
            FfiConverterULong.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: TrezorPrevTxOutput): ULong = (
            FfiConverterULong.allocationSize(value.`amount`) +
            FfiConverterString.allocationSize(value.`scriptPubkey`)
    )

    override fun write(value: TrezorPrevTxOutput, buf: ByteBuffer) {
        FfiConverterULong.write(value.`amount`, buf)
        FfiConverterString.write(value.`scriptPubkey`, buf)
    }
}




public object FfiConverterTypeTrezorPublicKeyResponse: FfiConverterRustBuffer<TrezorPublicKeyResponse> {
    override fun read(buf: ByteBuffer): TrezorPublicKeyResponse {
        return TrezorPublicKeyResponse(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterOptionalUInt.read(buf),
        )
    }

    override fun allocationSize(value: TrezorPublicKeyResponse): ULong = (
            FfiConverterString.allocationSize(value.`xpub`) +
            FfiConverterString.allocationSize(value.`path`) +
            FfiConverterString.allocationSize(value.`publicKey`) +
            FfiConverterString.allocationSize(value.`chainCode`) +
            FfiConverterUInt.allocationSize(value.`fingerprint`) +
            FfiConverterUInt.allocationSize(value.`depth`) +
            FfiConverterOptionalUInt.allocationSize(value.`rootFingerprint`)
    )

    override fun write(value: TrezorPublicKeyResponse, buf: ByteBuffer) {
        FfiConverterString.write(value.`xpub`, buf)
        FfiConverterString.write(value.`path`, buf)
        FfiConverterString.write(value.`publicKey`, buf)
        FfiConverterString.write(value.`chainCode`, buf)
        FfiConverterUInt.write(value.`fingerprint`, buf)
        FfiConverterUInt.write(value.`depth`, buf)
        FfiConverterOptionalUInt.write(value.`rootFingerprint`, buf)
    }
}




public object FfiConverterTypeTrezorSignMessageParams: FfiConverterRustBuffer<TrezorSignMessageParams> {
    override fun read(buf: ByteBuffer): TrezorSignMessageParams {
        return TrezorSignMessageParams(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalTypeTrezorCoinType.read(buf),
        )
    }

    override fun allocationSize(value: TrezorSignMessageParams): ULong = (
            FfiConverterString.allocationSize(value.`path`) +
            FfiConverterString.allocationSize(value.`message`) +
            FfiConverterOptionalTypeTrezorCoinType.allocationSize(value.`coin`)
    )

    override fun write(value: TrezorSignMessageParams, buf: ByteBuffer) {
        FfiConverterString.write(value.`path`, buf)
        FfiConverterString.write(value.`message`, buf)
        FfiConverterOptionalTypeTrezorCoinType.write(value.`coin`, buf)
    }
}




public object FfiConverterTypeTrezorSignTxParams: FfiConverterRustBuffer<TrezorSignTxParams> {
    override fun read(buf: ByteBuffer): TrezorSignTxParams {
        return TrezorSignTxParams(
            FfiConverterSequenceTypeTrezorTxInput.read(buf),
            FfiConverterSequenceTypeTrezorTxOutput.read(buf),
            FfiConverterOptionalTypeTrezorCoinType.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterSequenceTypeTrezorPrevTx.read(buf),
        )
    }

    override fun allocationSize(value: TrezorSignTxParams): ULong = (
            FfiConverterSequenceTypeTrezorTxInput.allocationSize(value.`inputs`) +
            FfiConverterSequenceTypeTrezorTxOutput.allocationSize(value.`outputs`) +
            FfiConverterOptionalTypeTrezorCoinType.allocationSize(value.`coin`) +
            FfiConverterOptionalUInt.allocationSize(value.`lockTime`) +
            FfiConverterOptionalUInt.allocationSize(value.`version`) +
            FfiConverterSequenceTypeTrezorPrevTx.allocationSize(value.`prevTxs`)
    )

    override fun write(value: TrezorSignTxParams, buf: ByteBuffer) {
        FfiConverterSequenceTypeTrezorTxInput.write(value.`inputs`, buf)
        FfiConverterSequenceTypeTrezorTxOutput.write(value.`outputs`, buf)
        FfiConverterOptionalTypeTrezorCoinType.write(value.`coin`, buf)
        FfiConverterOptionalUInt.write(value.`lockTime`, buf)
        FfiConverterOptionalUInt.write(value.`version`, buf)
        FfiConverterSequenceTypeTrezorPrevTx.write(value.`prevTxs`, buf)
    }
}




public object FfiConverterTypeTrezorSignedMessageResponse: FfiConverterRustBuffer<TrezorSignedMessageResponse> {
    override fun read(buf: ByteBuffer): TrezorSignedMessageResponse {
        return TrezorSignedMessageResponse(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: TrezorSignedMessageResponse): ULong = (
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterString.allocationSize(value.`signature`)
    )

    override fun write(value: TrezorSignedMessageResponse, buf: ByteBuffer) {
        FfiConverterString.write(value.`address`, buf)
        FfiConverterString.write(value.`signature`, buf)
    }
}




public object FfiConverterTypeTrezorSignedTx: FfiConverterRustBuffer<TrezorSignedTx> {
    override fun read(buf: ByteBuffer): TrezorSignedTx {
        return TrezorSignedTx(
            FfiConverterSequenceString.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: TrezorSignedTx): ULong = (
            FfiConverterSequenceString.allocationSize(value.`signatures`) +
            FfiConverterString.allocationSize(value.`serializedTx`)
    )

    override fun write(value: TrezorSignedTx, buf: ByteBuffer) {
        FfiConverterSequenceString.write(value.`signatures`, buf)
        FfiConverterString.write(value.`serializedTx`, buf)
    }
}




public object FfiConverterTypeTrezorTransportReadResult: FfiConverterRustBuffer<TrezorTransportReadResult> {
    override fun read(buf: ByteBuffer): TrezorTransportReadResult {
        return TrezorTransportReadResult(
            FfiConverterBoolean.read(buf),
            FfiConverterByteArray.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: TrezorTransportReadResult): ULong = (
            FfiConverterBoolean.allocationSize(value.`success`) +
            FfiConverterByteArray.allocationSize(value.`data`) +
            FfiConverterString.allocationSize(value.`error`)
    )

    override fun write(value: TrezorTransportReadResult, buf: ByteBuffer) {
        FfiConverterBoolean.write(value.`success`, buf)
        FfiConverterByteArray.write(value.`data`, buf)
        FfiConverterString.write(value.`error`, buf)
    }
}




public object FfiConverterTypeTrezorTransportWriteResult: FfiConverterRustBuffer<TrezorTransportWriteResult> {
    override fun read(buf: ByteBuffer): TrezorTransportWriteResult {
        return TrezorTransportWriteResult(
            FfiConverterBoolean.read(buf),
            FfiConverterString.read(buf),
        )
    }

    override fun allocationSize(value: TrezorTransportWriteResult): ULong = (
            FfiConverterBoolean.allocationSize(value.`success`) +
            FfiConverterString.allocationSize(value.`error`)
    )

    override fun write(value: TrezorTransportWriteResult, buf: ByteBuffer) {
        FfiConverterBoolean.write(value.`success`, buf)
        FfiConverterString.write(value.`error`, buf)
    }
}




public object FfiConverterTypeTrezorTxInput: FfiConverterRustBuffer<TrezorTxInput> {
    override fun read(buf: ByteBuffer): TrezorTxInput {
        return TrezorTxInput(
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterTypeTrezorScriptType.read(buf),
            FfiConverterOptionalUInt.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalUInt.read(buf),
        )
    }

    override fun allocationSize(value: TrezorTxInput): ULong = (
            FfiConverterString.allocationSize(value.`prevHash`) +
            FfiConverterUInt.allocationSize(value.`prevIndex`) +
            FfiConverterString.allocationSize(value.`path`) +
            FfiConverterULong.allocationSize(value.`amount`) +
            FfiConverterTypeTrezorScriptType.allocationSize(value.`scriptType`) +
            FfiConverterOptionalUInt.allocationSize(value.`sequence`) +
            FfiConverterOptionalString.allocationSize(value.`origHash`) +
            FfiConverterOptionalUInt.allocationSize(value.`origIndex`)
    )

    override fun write(value: TrezorTxInput, buf: ByteBuffer) {
        FfiConverterString.write(value.`prevHash`, buf)
        FfiConverterUInt.write(value.`prevIndex`, buf)
        FfiConverterString.write(value.`path`, buf)
        FfiConverterULong.write(value.`amount`, buf)
        FfiConverterTypeTrezorScriptType.write(value.`scriptType`, buf)
        FfiConverterOptionalUInt.write(value.`sequence`, buf)
        FfiConverterOptionalString.write(value.`origHash`, buf)
        FfiConverterOptionalUInt.write(value.`origIndex`, buf)
    }
}




public object FfiConverterTypeTrezorTxOutput: FfiConverterRustBuffer<TrezorTxOutput> {
    override fun read(buf: ByteBuffer): TrezorTxOutput {
        return TrezorTxOutput(
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterULong.read(buf),
            FfiConverterOptionalTypeTrezorScriptType.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalUInt.read(buf),
        )
    }

    override fun allocationSize(value: TrezorTxOutput): ULong = (
            FfiConverterOptionalString.allocationSize(value.`address`) +
            FfiConverterOptionalString.allocationSize(value.`path`) +
            FfiConverterULong.allocationSize(value.`amount`) +
            FfiConverterOptionalTypeTrezorScriptType.allocationSize(value.`scriptType`) +
            FfiConverterOptionalString.allocationSize(value.`opReturnData`) +
            FfiConverterOptionalString.allocationSize(value.`origHash`) +
            FfiConverterOptionalUInt.allocationSize(value.`origIndex`)
    )

    override fun write(value: TrezorTxOutput, buf: ByteBuffer) {
        FfiConverterOptionalString.write(value.`address`, buf)
        FfiConverterOptionalString.write(value.`path`, buf)
        FfiConverterULong.write(value.`amount`, buf)
        FfiConverterOptionalTypeTrezorScriptType.write(value.`scriptType`, buf)
        FfiConverterOptionalString.write(value.`opReturnData`, buf)
        FfiConverterOptionalString.write(value.`origHash`, buf)
        FfiConverterOptionalUInt.write(value.`origIndex`, buf)
    }
}




public object FfiConverterTypeTrezorVerifyMessageParams: FfiConverterRustBuffer<TrezorVerifyMessageParams> {
    override fun read(buf: ByteBuffer): TrezorVerifyMessageParams {
        return TrezorVerifyMessageParams(
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterString.read(buf),
            FfiConverterOptionalTypeTrezorCoinType.read(buf),
        )
    }

    override fun allocationSize(value: TrezorVerifyMessageParams): ULong = (
            FfiConverterString.allocationSize(value.`address`) +
            FfiConverterString.allocationSize(value.`signature`) +
            FfiConverterString.allocationSize(value.`message`) +
            FfiConverterOptionalTypeTrezorCoinType.allocationSize(value.`coin`)
    )

    override fun write(value: TrezorVerifyMessageParams, buf: ByteBuffer) {
        FfiConverterString.write(value.`address`, buf)
        FfiConverterString.write(value.`signature`, buf)
        FfiConverterString.write(value.`message`, buf)
        FfiConverterOptionalTypeTrezorCoinType.write(value.`coin`, buf)
    }
}




public object FfiConverterTypeTxInput: FfiConverterRustBuffer<TxInput> {
    override fun read(buf: ByteBuffer): TxInput {
        return TxInput(
            FfiConverterString.read(buf),
            FfiConverterUInt.read(buf),
            FfiConverterString.read(buf),
            FfiConverterSequenceString.read(buf),
            FfiConverterUInt.read(buf),
        )
    }

    override fun allocationSize(value: TxInput): ULong = (
            FfiConverterString.allocationSize(value.`txid`) +
            FfiConverterUInt.allocationSize(value.`vout`) +
            FfiConverterString.allocationSize(value.`scriptsig`) +
            FfiConverterSequenceString.allocationSize(value.`witness`) +
            FfiConverterUInt.allocationSize(value.`sequence`)
    )

    override fun write(value: TxInput, buf: ByteBuffer) {
        FfiConverterString.write(value.`txid`, buf)
        FfiConverterUInt.write(value.`vout`, buf)
        FfiConverterString.write(value.`scriptsig`, buf)
        FfiConverterSequenceString.write(value.`witness`, buf)
        FfiConverterUInt.write(value.`sequence`, buf)
    }
}




public object FfiConverterTypeTxOutput: FfiConverterRustBuffer<TxOutput> {
    override fun read(buf: ByteBuffer): TxOutput {
        return TxOutput(
            FfiConverterString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterOptionalString.read(buf),
            FfiConverterLong.read(buf),
            FfiConverterUInt.read(buf),
        )
    }

    override fun allocationSize(value: TxOutput): ULong = (
            FfiConverterString.allocationSize(value.`scriptpubkey`) +
            FfiConverterOptionalString.allocationSize(value.`scriptpubkeyType`) +
            FfiConverterOptionalString.allocationSize(value.`scriptpubkeyAddress`) +
            FfiConverterLong.allocationSize(value.`value`) +
            FfiConverterUInt.allocationSize(value.`n`)
    )

    override fun write(value: TxOutput, buf: ByteBuffer) {
        FfiConverterString.write(value.`scriptpubkey`, buf)
        FfiConverterOptionalString.write(value.`scriptpubkeyType`, buf)
        FfiConverterOptionalString.write(value.`scriptpubkeyAddress`, buf)
        FfiConverterLong.write(value.`value`, buf)
        FfiConverterUInt.write(value.`n`, buf)
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




public object SweepExceptionErrorHandler : UniffiRustCallStatusErrorHandler<SweepException> {
    override fun lift(errorBuf: RustBufferByValue): SweepException = FfiConverterTypeSweepError.lift(errorBuf)
}

public object FfiConverterTypeSweepError : FfiConverterRustBuffer<SweepException> {
    override fun read(buf: ByteBuffer): SweepException {
        return when (buf.getInt()) {
            1 -> SweepException.SweepFailed(
                FfiConverterString.read(buf),
                )
            2 -> SweepException.NoUtxosFound()
            3 -> SweepException.InvalidMnemonic()
            else -> throw RuntimeException("invalid error enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: SweepException): ULong {
        return when (value) {
            is SweepException.SweepFailed -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.v1)
            )
            is SweepException.NoUtxosFound -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is SweepException.InvalidMnemonic -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
        }
    }

    override fun write(value: SweepException, buf: ByteBuffer) {
        when (value) {
            is SweepException.SweepFailed -> {
                buf.putInt(1)
                FfiConverterString.write(value.v1, buf)
                Unit
            }
            is SweepException.NoUtxosFound -> {
                buf.putInt(2)
                Unit
            }
            is SweepException.InvalidMnemonic -> {
                buf.putInt(3)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}





public object FfiConverterTypeTrezorCoinType: FfiConverterRustBuffer<TrezorCoinType> {
    override fun read(buf: ByteBuffer): TrezorCoinType = try {
        TrezorCoinType.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: TrezorCoinType): ULong = 4UL

    override fun write(value: TrezorCoinType, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}




public object TrezorExceptionErrorHandler : UniffiRustCallStatusErrorHandler<TrezorException> {
    override fun lift(errorBuf: RustBufferByValue): TrezorException = FfiConverterTypeTrezorError.lift(errorBuf)
}

public object FfiConverterTypeTrezorError : FfiConverterRustBuffer<TrezorException> {
    override fun read(buf: ByteBuffer): TrezorException {
        return when (buf.getInt()) {
            1 -> TrezorException.TransportException(
                FfiConverterString.read(buf),
                )
            2 -> TrezorException.DeviceNotFound()
            3 -> TrezorException.DeviceDisconnected()
            4 -> TrezorException.ConnectionException(
                FfiConverterString.read(buf),
                )
            5 -> TrezorException.ProtocolException(
                FfiConverterString.read(buf),
                )
            6 -> TrezorException.PairingRequired()
            7 -> TrezorException.PairingFailed(
                FfiConverterString.read(buf),
                )
            8 -> TrezorException.PinRequired()
            9 -> TrezorException.PinCancelled()
            10 -> TrezorException.InvalidPin()
            11 -> TrezorException.PassphraseRequired()
            12 -> TrezorException.UserCancelled()
            13 -> TrezorException.Timeout()
            14 -> TrezorException.InvalidPath(
                FfiConverterString.read(buf),
                )
            15 -> TrezorException.DeviceException(
                FfiConverterString.read(buf),
                )
            16 -> TrezorException.NotInitialized()
            17 -> TrezorException.NotConnected()
            18 -> TrezorException.SessionException(
                FfiConverterString.read(buf),
                )
            19 -> TrezorException.IoException(
                FfiConverterString.read(buf),
                )
            else -> throw RuntimeException("invalid error enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: TrezorException): ULong {
        return when (value) {
            is TrezorException.TransportException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is TrezorException.DeviceNotFound -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is TrezorException.DeviceDisconnected -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is TrezorException.ConnectionException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is TrezorException.ProtocolException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is TrezorException.PairingRequired -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is TrezorException.PairingFailed -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is TrezorException.PinRequired -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is TrezorException.PinCancelled -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is TrezorException.InvalidPin -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is TrezorException.PassphraseRequired -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is TrezorException.UserCancelled -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is TrezorException.Timeout -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is TrezorException.InvalidPath -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is TrezorException.DeviceException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is TrezorException.NotInitialized -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is TrezorException.NotConnected -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
            )
            is TrezorException.SessionException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
            is TrezorException.IoException -> (
                // Add the size for the Int that specifies the variant plus the size needed for all fields
                4UL
                + FfiConverterString.allocationSize(value.`errorDetails`)
            )
        }
    }

    override fun write(value: TrezorException, buf: ByteBuffer) {
        when (value) {
            is TrezorException.TransportException -> {
                buf.putInt(1)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is TrezorException.DeviceNotFound -> {
                buf.putInt(2)
                Unit
            }
            is TrezorException.DeviceDisconnected -> {
                buf.putInt(3)
                Unit
            }
            is TrezorException.ConnectionException -> {
                buf.putInt(4)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is TrezorException.ProtocolException -> {
                buf.putInt(5)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is TrezorException.PairingRequired -> {
                buf.putInt(6)
                Unit
            }
            is TrezorException.PairingFailed -> {
                buf.putInt(7)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is TrezorException.PinRequired -> {
                buf.putInt(8)
                Unit
            }
            is TrezorException.PinCancelled -> {
                buf.putInt(9)
                Unit
            }
            is TrezorException.InvalidPin -> {
                buf.putInt(10)
                Unit
            }
            is TrezorException.PassphraseRequired -> {
                buf.putInt(11)
                Unit
            }
            is TrezorException.UserCancelled -> {
                buf.putInt(12)
                Unit
            }
            is TrezorException.Timeout -> {
                buf.putInt(13)
                Unit
            }
            is TrezorException.InvalidPath -> {
                buf.putInt(14)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is TrezorException.DeviceException -> {
                buf.putInt(15)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is TrezorException.NotInitialized -> {
                buf.putInt(16)
                Unit
            }
            is TrezorException.NotConnected -> {
                buf.putInt(17)
                Unit
            }
            is TrezorException.SessionException -> {
                buf.putInt(18)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
            is TrezorException.IoException -> {
                buf.putInt(19)
                FfiConverterString.write(value.`errorDetails`, buf)
                Unit
            }
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}





public object FfiConverterTypeTrezorScriptType: FfiConverterRustBuffer<TrezorScriptType> {
    override fun read(buf: ByteBuffer): TrezorScriptType = try {
        TrezorScriptType.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: TrezorScriptType): ULong = 4UL

    override fun write(value: TrezorScriptType, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}





public object FfiConverterTypeTrezorTransportType: FfiConverterRustBuffer<TrezorTransportType> {
    override fun read(buf: ByteBuffer): TrezorTransportType = try {
        TrezorTransportType.entries[buf.getInt() - 1]
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: TrezorTransportType): ULong = 4UL

    override fun write(value: TrezorTransportType, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
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




public object FfiConverterOptionalUShort: FfiConverterRustBuffer<kotlin.UShort?> {
    override fun read(buf: ByteBuffer): kotlin.UShort? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterUShort.read(buf)
    }

    override fun allocationSize(value: kotlin.UShort?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterUShort.allocationSize(value)
        }
    }

    override fun write(value: kotlin.UShort?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterUShort.write(value, buf)
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




public object FfiConverterOptionalTypeOnchainActivity: FfiConverterRustBuffer<OnchainActivity?> {
    override fun read(buf: ByteBuffer): OnchainActivity? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeOnchainActivity.read(buf)
    }

    override fun allocationSize(value: OnchainActivity?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeOnchainActivity.allocationSize(value)
        }
    }

    override fun write(value: OnchainActivity?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeOnchainActivity.write(value, buf)
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




public object FfiConverterOptionalTypeTransactionDetails: FfiConverterRustBuffer<TransactionDetails?> {
    override fun read(buf: ByteBuffer): TransactionDetails? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeTransactionDetails.read(buf)
    }

    override fun allocationSize(value: TransactionDetails?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeTransactionDetails.allocationSize(value)
        }
    }

    override fun write(value: TransactionDetails?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeTransactionDetails.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeTrezorCallMessageResult: FfiConverterRustBuffer<TrezorCallMessageResult?> {
    override fun read(buf: ByteBuffer): TrezorCallMessageResult? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeTrezorCallMessageResult.read(buf)
    }

    override fun allocationSize(value: TrezorCallMessageResult?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeTrezorCallMessageResult.allocationSize(value)
        }
    }

    override fun write(value: TrezorCallMessageResult?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeTrezorCallMessageResult.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeTrezorDeviceInfo: FfiConverterRustBuffer<TrezorDeviceInfo?> {
    override fun read(buf: ByteBuffer): TrezorDeviceInfo? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeTrezorDeviceInfo.read(buf)
    }

    override fun allocationSize(value: TrezorDeviceInfo?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeTrezorDeviceInfo.allocationSize(value)
        }
    }

    override fun write(value: TrezorDeviceInfo?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeTrezorDeviceInfo.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeTrezorFeatures: FfiConverterRustBuffer<TrezorFeatures?> {
    override fun read(buf: ByteBuffer): TrezorFeatures? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeTrezorFeatures.read(buf)
    }

    override fun allocationSize(value: TrezorFeatures?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeTrezorFeatures.allocationSize(value)
        }
    }

    override fun write(value: TrezorFeatures?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeTrezorFeatures.write(value, buf)
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




public object FfiConverterOptionalTypeTrezorCoinType: FfiConverterRustBuffer<TrezorCoinType?> {
    override fun read(buf: ByteBuffer): TrezorCoinType? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeTrezorCoinType.read(buf)
    }

    override fun allocationSize(value: TrezorCoinType?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeTrezorCoinType.allocationSize(value)
        }
    }

    override fun write(value: TrezorCoinType?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeTrezorCoinType.write(value, buf)
        }
    }
}




public object FfiConverterOptionalTypeTrezorScriptType: FfiConverterRustBuffer<TrezorScriptType?> {
    override fun read(buf: ByteBuffer): TrezorScriptType? {
        if (buf.get().toInt() == 0) {
            return null
        }
        return FfiConverterTypeTrezorScriptType.read(buf)
    }

    override fun allocationSize(value: TrezorScriptType?): ULong {
        if (value == null) {
            return 1UL
        } else {
            return 1UL + FfiConverterTypeTrezorScriptType.allocationSize(value)
        }
    }

    override fun write(value: TrezorScriptType?, buf: ByteBuffer) {
        if (value == null) {
            buf.put(0)
        } else {
            buf.put(1)
            FfiConverterTypeTrezorScriptType.write(value, buf)
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




public object FfiConverterSequenceTypeNativeDeviceInfo: FfiConverterRustBuffer<List<NativeDeviceInfo>> {
    override fun read(buf: ByteBuffer): List<NativeDeviceInfo> {
        val len = buf.getInt()
        return List<NativeDeviceInfo>(len) {
            FfiConverterTypeNativeDeviceInfo.read(buf)
        }
    }

    override fun allocationSize(value: List<NativeDeviceInfo>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeNativeDeviceInfo.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<NativeDeviceInfo>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeNativeDeviceInfo.write(it, buf)
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




public object FfiConverterSequenceTypeTransactionDetails: FfiConverterRustBuffer<List<TransactionDetails>> {
    override fun read(buf: ByteBuffer): List<TransactionDetails> {
        val len = buf.getInt()
        return List<TransactionDetails>(len) {
            FfiConverterTypeTransactionDetails.read(buf)
        }
    }

    override fun allocationSize(value: List<TransactionDetails>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeTransactionDetails.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<TransactionDetails>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeTransactionDetails.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeTrezorDeviceInfo: FfiConverterRustBuffer<List<TrezorDeviceInfo>> {
    override fun read(buf: ByteBuffer): List<TrezorDeviceInfo> {
        val len = buf.getInt()
        return List<TrezorDeviceInfo>(len) {
            FfiConverterTypeTrezorDeviceInfo.read(buf)
        }
    }

    override fun allocationSize(value: List<TrezorDeviceInfo>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeTrezorDeviceInfo.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<TrezorDeviceInfo>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeTrezorDeviceInfo.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeTrezorPrevTx: FfiConverterRustBuffer<List<TrezorPrevTx>> {
    override fun read(buf: ByteBuffer): List<TrezorPrevTx> {
        val len = buf.getInt()
        return List<TrezorPrevTx>(len) {
            FfiConverterTypeTrezorPrevTx.read(buf)
        }
    }

    override fun allocationSize(value: List<TrezorPrevTx>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeTrezorPrevTx.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<TrezorPrevTx>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeTrezorPrevTx.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeTrezorPrevTxInput: FfiConverterRustBuffer<List<TrezorPrevTxInput>> {
    override fun read(buf: ByteBuffer): List<TrezorPrevTxInput> {
        val len = buf.getInt()
        return List<TrezorPrevTxInput>(len) {
            FfiConverterTypeTrezorPrevTxInput.read(buf)
        }
    }

    override fun allocationSize(value: List<TrezorPrevTxInput>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeTrezorPrevTxInput.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<TrezorPrevTxInput>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeTrezorPrevTxInput.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeTrezorPrevTxOutput: FfiConverterRustBuffer<List<TrezorPrevTxOutput>> {
    override fun read(buf: ByteBuffer): List<TrezorPrevTxOutput> {
        val len = buf.getInt()
        return List<TrezorPrevTxOutput>(len) {
            FfiConverterTypeTrezorPrevTxOutput.read(buf)
        }
    }

    override fun allocationSize(value: List<TrezorPrevTxOutput>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeTrezorPrevTxOutput.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<TrezorPrevTxOutput>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeTrezorPrevTxOutput.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeTrezorTxInput: FfiConverterRustBuffer<List<TrezorTxInput>> {
    override fun read(buf: ByteBuffer): List<TrezorTxInput> {
        val len = buf.getInt()
        return List<TrezorTxInput>(len) {
            FfiConverterTypeTrezorTxInput.read(buf)
        }
    }

    override fun allocationSize(value: List<TrezorTxInput>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeTrezorTxInput.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<TrezorTxInput>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeTrezorTxInput.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeTrezorTxOutput: FfiConverterRustBuffer<List<TrezorTxOutput>> {
    override fun read(buf: ByteBuffer): List<TrezorTxOutput> {
        val len = buf.getInt()
        return List<TrezorTxOutput>(len) {
            FfiConverterTypeTrezorTxOutput.read(buf)
        }
    }

    override fun allocationSize(value: List<TrezorTxOutput>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeTrezorTxOutput.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<TrezorTxOutput>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeTrezorTxOutput.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeTxInput: FfiConverterRustBuffer<List<TxInput>> {
    override fun read(buf: ByteBuffer): List<TxInput> {
        val len = buf.getInt()
        return List<TxInput>(len) {
            FfiConverterTypeTxInput.read(buf)
        }
    }

    override fun allocationSize(value: List<TxInput>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeTxInput.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<TxInput>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeTxInput.write(it, buf)
        }
    }
}




public object FfiConverterSequenceTypeTxOutput: FfiConverterRustBuffer<List<TxOutput>> {
    override fun read(buf: ByteBuffer): List<TxOutput> {
        val len = buf.getInt()
        return List<TxOutput>(len) {
            FfiConverterTypeTxOutput.read(buf)
        }
    }

    override fun allocationSize(value: List<TxOutput>): ULong {
        val sizeForLength = 4UL
        val sizeForItems = value.sumOf { FfiConverterTypeTxOutput.allocationSize(it) }
        return sizeForLength + sizeForItems
    }

    override fun write(value: List<TxOutput>, buf: ByteBuffer) {
        buf.putInt(value.size)
        value.iterator().forEach {
            FfiConverterTypeTxOutput.write(it, buf)
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

@Throws(SweepException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `broadcastSweepTransaction`(`psbt`: kotlin.String, `mnemonicPhrase`: kotlin.String, `network`: Network?, `bip39Passphrase`: kotlin.String?, `electrumUrl`: kotlin.String): SweepResult {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_broadcast_sweep_transaction(
            FfiConverterString.lower(`psbt`),
            FfiConverterString.lower(`mnemonicPhrase`),
            FfiConverterOptionalTypeNetwork.lower(`network`),
            FfiConverterOptionalString.lower(`bip39Passphrase`),
            FfiConverterString.lower(`electrumUrl`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeSweepResult.lift(it) },
        // Error FFI converter
        SweepExceptionErrorHandler,
    )
}

public fun `calculateChannelLiquidityOptions`(`params`: ChannelLiquidityParams): ChannelLiquidityOptions {
    return FfiConverterTypeChannelLiquidityOptions.lift(uniffiRustCall { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_calculate_channel_liquidity_options(
            FfiConverterTypeChannelLiquidityParams.lower(`params`),
            uniffiRustCallStatus,
        )
    })
}

@Throws(SweepException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `checkSweepableBalances`(`mnemonicPhrase`: kotlin.String, `network`: Network?, `bip39Passphrase`: kotlin.String?, `electrumUrl`: kotlin.String): SweepableBalances {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_check_sweepable_balances(
            FfiConverterString.lower(`mnemonicPhrase`),
            FfiConverterOptionalTypeNetwork.lower(`network`),
            FfiConverterOptionalString.lower(`bip39Passphrase`),
            FfiConverterString.lower(`electrumUrl`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeSweepableBalances.lift(it) },
        // Error FFI converter
        SweepExceptionErrorHandler,
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

@Throws(ActivityException::class)
public fun `deleteTransactionDetails`(`txId`: kotlin.String): kotlin.Boolean {
    return FfiConverterBoolean.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_delete_transaction_details(
            FfiConverterString.lower(`txId`),
            uniffiRustCallStatus,
        )
    })
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
public fun `getActivityByTxId`(`txId`: kotlin.String): OnchainActivity? {
    return FfiConverterOptionalTypeOnchainActivity.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_activity_by_tx_id(
            FfiConverterString.lower(`txId`),
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
public fun `getAllTransactionDetails`(): List<TransactionDetails> {
    return FfiConverterSequenceTypeTransactionDetails.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_all_transaction_details(
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

public fun `getDefaultLspBalance`(`params`: DefaultLspBalanceParams): kotlin.ULong {
    return FfiConverterULong.lift(uniffiRustCall { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_default_lsp_balance(
            FfiConverterTypeDefaultLspBalanceParams.lower(`params`),
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

@Throws(ActivityException::class)
public fun `getTransactionDetails`(`txId`: kotlin.String): TransactionDetails? {
    return FfiConverterOptionalTypeTransactionDetails.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_get_transaction_details(
            FfiConverterString.lower(`txId`),
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

@Throws(ActivityException::class)
public fun `isAddressUsed`(`address`: kotlin.String): kotlin.Boolean {
    return FfiConverterBoolean.lift(uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_is_address_used(
            FfiConverterString.lower(`address`),
            uniffiRustCallStatus,
        )
    })
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

@Throws(ActivityException::class)
public fun `markActivityAsSeen`(`activityId`: kotlin.String, `seenAt`: kotlin.ULong) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_mark_activity_as_seen(
            FfiConverterString.lower(`activityId`),
            FfiConverterULong.lower(`seenAt`),
            uniffiRustCallStatus,
        )
    }
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

@Throws(SweepException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `prepareSweepTransaction`(`mnemonicPhrase`: kotlin.String, `network`: Network?, `bip39Passphrase`: kotlin.String?, `electrumUrl`: kotlin.String, `destinationAddress`: kotlin.String, `feeRateSatsPerVbyte`: kotlin.UInt?): SweepTransactionPreview {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_prepare_sweep_transaction(
            FfiConverterString.lower(`mnemonicPhrase`),
            FfiConverterOptionalTypeNetwork.lower(`network`),
            FfiConverterOptionalString.lower(`bip39Passphrase`),
            FfiConverterString.lower(`electrumUrl`),
            FfiConverterString.lower(`destinationAddress`),
            FfiConverterOptionalUInt.lower(`feeRateSatsPerVbyte`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeSweepTransactionPreview.lift(it) },
        // Error FFI converter
        SweepExceptionErrorHandler,
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

/**
 * Clear stored Bluetooth pairing credentials for a specific Trezor device.
 *
 * This removes any stored credentials, requiring re-pairing on the next connection.
 * Useful when a device has been reset or credentials have become stale.
 */
@Throws(TrezorException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `trezorClearCredentials`(`deviceId`: kotlin.String) {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_clear_credentials(
            FfiConverterString.lower(`deviceId`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_void(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_void(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_void(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_void(future) },
        // lift function
        { Unit },
        
        // Error FFI converter
        TrezorExceptionErrorHandler,
    )
}

/**
 * Connect to a Trezor device by its ID.
 *
 * For Bluetooth devices, this will use stored credentials if available,
 * or trigger pairing if needed.
 */
@Throws(TrezorException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `trezorConnect`(`deviceId`: kotlin.String): TrezorFeatures {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_connect(
            FfiConverterString.lower(`deviceId`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeTrezorFeatures.lift(it) },
        // Error FFI converter
        TrezorExceptionErrorHandler,
    )
}

/**
 * Disconnect from the currently connected Trezor device.
 */
@Throws(TrezorException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `trezorDisconnect`() {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_disconnect(
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_void(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_void(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_void(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_void(future) },
        // lift function
        { Unit },
        
        // Error FFI converter
        TrezorExceptionErrorHandler,
    )
}

/**
 * Get a Bitcoin address from the connected Trezor device.
 */
@Throws(TrezorException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `trezorGetAddress`(`params`: TrezorGetAddressParams): TrezorAddressResponse {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_get_address(
            FfiConverterTypeTrezorGetAddressParams.lower(`params`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeTrezorAddressResponse.lift(it) },
        // Error FFI converter
        TrezorExceptionErrorHandler,
    )
}

/**
 * Get information about the currently connected Trezor device.
 */
public suspend fun `trezorGetConnectedDevice`(): TrezorDeviceInfo? {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_get_connected_device(
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterOptionalTypeTrezorDeviceInfo.lift(it) },
        // Error FFI converter
        UniffiNullRustCallStatusErrorHandler,
    )
}

/**
 * Get the device's master root fingerprint as an 8-character hex string.
 *
 * Returns the root fingerprint in the standard descriptor format (e.g., "73c5da0a").
 * Requires a connected device.
 */
@Throws(TrezorException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `trezorGetDeviceFingerprint`(): kotlin.String {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_get_device_fingerprint(
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterString.lift(it) },
        // Error FFI converter
        TrezorExceptionErrorHandler,
    )
}

/**
 * Get the cached features of the currently connected Trezor device.
 *
 * Returns the features that were obtained during `trezor_connect()`, without
 * triggering any device interaction. Returns None if no device is connected.
 */
public suspend fun `trezorGetFeatures`(): TrezorFeatures? {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_get_features(
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterOptionalTypeTrezorFeatures.lift(it) },
        // Error FFI converter
        UniffiNullRustCallStatusErrorHandler,
    )
}

/**
 * Get a public key (xpub) from the connected Trezor device.
 */
@Throws(TrezorException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `trezorGetPublicKey`(`params`: TrezorGetPublicKeyParams): TrezorPublicKeyResponse {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_get_public_key(
            FfiConverterTypeTrezorGetPublicKeyParams.lower(`params`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeTrezorPublicKeyResponse.lift(it) },
        // Error FFI converter
        TrezorExceptionErrorHandler,
    )
}

/**
 * Initialize the Trezor manager with optional credential storage.
 *
 * The credential_path is used to persist Bluetooth pairing credentials,
 * allowing reconnection without re-pairing.
 *
 * NOTE: On Android, you must call the native initBle() function first!
 */
@Throws(TrezorException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `trezorInitialize`(`credentialPath`: kotlin.String?) {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_initialize(
            FfiConverterOptionalString.lower(`credentialPath`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_void(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_void(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_void(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_void(future) },
        // lift function
        { Unit },
        
        // Error FFI converter
        TrezorExceptionErrorHandler,
    )
}

/**
 * Check if BLE has been initialized.
 *
 * On Android: Returns true if BluetoothInit.nativeInit() was called successfully.
 * On other platforms: Always returns true (BLE works natively).
 */
public fun `trezorIsBleAvailable`(): kotlin.Boolean {
    return FfiConverterBoolean.lift(uniffiRustCall { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_is_ble_available(
            uniffiRustCallStatus,
        )
    })
}

/**
 * Check if a Trezor device is currently connected.
 */
public suspend fun `trezorIsConnected`(): kotlin.Boolean {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_is_connected(
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_i8(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_i8(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_i8(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_i8(future) },
        // lift function
        { FfiConverterBoolean.lift(it) },
        // Error FFI converter
        UniffiNullRustCallStatusErrorHandler,
    )
}

/**
 * Check if the Trezor manager is initialized.
 */
public suspend fun `trezorIsInitialized`(): kotlin.Boolean {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_is_initialized(
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_i8(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_i8(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_i8(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_i8(future) },
        // lift function
        { FfiConverterBoolean.lift(it) },
        // Error FFI converter
        UniffiNullRustCallStatusErrorHandler,
    )
}

/**
 * List previously discovered devices without triggering a new scan.
 */
@Throws(TrezorException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `trezorListDevices`(): List<TrezorDeviceInfo> {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_list_devices(
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterSequenceTypeTrezorDeviceInfo.lift(it) },
        // Error FFI converter
        TrezorExceptionErrorHandler,
    )
}

/**
 * Scan for available Trezor devices (USB + Bluetooth).
 *
 * This performs an active Bluetooth scan and enumerates USB devices.
 * Returns a list of discovered devices.
 */
@Throws(TrezorException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `trezorScan`(): List<TrezorDeviceInfo> {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_scan(
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterSequenceTypeTrezorDeviceInfo.lift(it) },
        // Error FFI converter
        TrezorExceptionErrorHandler,
    )
}

/**
 * Initialize the Trezor transport with a native callback implementation
 *
 * This must be called before any Trezor scanning/connection operations.
 * The native layer (iOS/Android) must implement the TrezorTransportCallback interface.
 */
public fun `trezorSetTransportCallback`(`callback`: TrezorTransportCallback) {
    uniffiRustCall { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_set_transport_callback(
            FfiConverterTypeTrezorTransportCallback.lower(`callback`),
            uniffiRustCallStatus,
        )
    }
}

/**
 * Set the UI callback for handling PIN and passphrase requests.
 *
 * This should be called before connecting to a Trezor device if you want
 * the library to handle PIN/passphrase requests via your UI instead of
 * returning errors.
 */
public fun `trezorSetUiCallback`(`callback`: TrezorUiCallback) {
    uniffiRustCall { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_set_ui_callback(
            FfiConverterTypeTrezorUiCallback.lower(`callback`),
            uniffiRustCallStatus,
        )
    }
}

/**
 * Sign a message with the connected Trezor device.
 */
@Throws(TrezorException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `trezorSignMessage`(`params`: TrezorSignMessageParams): TrezorSignedMessageResponse {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_sign_message(
            FfiConverterTypeTrezorSignMessageParams.lower(`params`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeTrezorSignedMessageResponse.lift(it) },
        // Error FFI converter
        TrezorExceptionErrorHandler,
    )
}

/**
 * Sign a Bitcoin transaction with the connected Trezor device.
 */
@Throws(TrezorException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `trezorSignTx`(`params`: TrezorSignTxParams): TrezorSignedTx {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_sign_tx(
            FfiConverterTypeTrezorSignTxParams.lower(`params`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeTrezorSignedTx.lift(it) },
        // Error FFI converter
        TrezorExceptionErrorHandler,
    )
}

/**
 * Sign a Bitcoin transaction from a PSBT (base64-encoded).
 *
 * Parses the PSBT, extracts inputs/outputs/prev_txs, signs via the connected
 * Trezor device, and returns the signed transaction.
 *
 * # Arguments
 * * `psbt_base64` - Base64-encoded PSBT data
 * * `network` - Bitcoin network type. Defaults to Bitcoin (mainnet) if None.
 */
@Throws(TrezorException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `trezorSignTxFromPsbt`(`psbtBase64`: kotlin.String, `network`: TrezorCoinType?): TrezorSignedTx {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_sign_tx_from_psbt(
            FfiConverterString.lower(`psbtBase64`),
            FfiConverterOptionalTypeTrezorCoinType.lower(`network`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_rust_buffer(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_rust_buffer(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_rust_buffer(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_rust_buffer(future) },
        // lift function
        { FfiConverterTypeTrezorSignedTx.lift(it) },
        // Error FFI converter
        TrezorExceptionErrorHandler,
    )
}

/**
 * Verify a message signature with the connected Trezor device.
 */
@Throws(TrezorException::class, kotlin.coroutines.cancellation.CancellationException::class)
public suspend fun `trezorVerifyMessage`(`params`: TrezorVerifyMessageParams): kotlin.Boolean {
    return uniffiRustCallAsync(
        UniffiLib.uniffi_bitkitcore_fn_func_trezor_verify_message(
            FfiConverterTypeTrezorVerifyMessageParams.lower(`params`),
        ),
        { future, callback, continuation -> UniffiLib.ffi_bitkitcore_rust_future_poll_i8(future, callback, continuation) },
        { future, continuation -> UniffiLib.ffi_bitkitcore_rust_future_complete_i8(future, continuation) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_free_i8(future) },
        { future -> UniffiLib.ffi_bitkitcore_rust_future_cancel_i8(future) },
        // lift function
        { FfiConverterBoolean.lift(it) },
        // Error FFI converter
        TrezorExceptionErrorHandler,
    )
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

@Throws(ActivityException::class)
public fun `upsertTransactionDetails`(`detailsList`: List<TransactionDetails>) {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_upsert_transaction_details(
            FfiConverterSequenceTypeTransactionDetails.lower(`detailsList`),
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

@Throws(ActivityException::class)
public fun `wipeAllTransactionDetails`() {
    uniffiRustCallWithError(ActivityExceptionErrorHandler) { uniffiRustCallStatus ->
        UniffiLib.uniffi_bitkitcore_fn_func_wipe_all_transaction_details(
            uniffiRustCallStatus,
        )
    }
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