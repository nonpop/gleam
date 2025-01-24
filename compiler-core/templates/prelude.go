package gleam

import (
	"bytes"
	"encoding/binary"
	"fmt"
	"hash"
	"hash/fnv"
	"math"
)

const Use_Import byte = 0

var hostIsBigEndian = false // TODO: detect

func NewHash() hash.Hash32 {
	return fnv.New32()
}

type Type[T any] interface {
	Hash() uint32
	Equal(T) bool
}

type Dynamic_t struct {
	Value interface{ Hash() uint32 }
}

func (d Dynamic_t) Hash() uint32           { return d.Value.Hash() }
func (d Dynamic_t) Equal(o Dynamic_t) bool { return d.Value == o.Value }

func HashTuple(valueHashes ...uint32) uint32 {
	h := NewHash()
	for _, hash := range valueHashes {
		if _, err := h.Write([]byte{
			byte(hash),
			byte(hash >> 8),
			byte(hash >> 16),
			byte(hash >> 24),
		}); err != nil {
			panic(err)
		}
	}
	return h.Sum32()
}

func HashConstructor(tag uint32, valueHashes ...uint32) uint32 {
	h := NewHash()
	if _, err := h.Write([]byte{
		byte(tag),
		byte(tag >> 8),
		byte(tag >> 16),
		byte(tag >> 24),
	}); err != nil {
		panic(err)
	}
	for _, hash := range valueHashes {
		if _, err := h.Write([]byte{
			byte(hash),
			byte(hash >> 8),
			byte(hash >> 16),
			byte(hash >> 24),
		}); err != nil {
			panic(err)
		}
	}
	return h.Sum32()
}

type OrderedCollectionHasher struct {
	h hash.Hash32
}

func NewOrderedCollectionHasher() OrderedCollectionHasher {
	return OrderedCollectionHasher{h: NewHash()}
}

func (a *OrderedCollectionHasher) WriteHash(elemHash uint32) {
	if _, err := a.h.Write([]byte{
		byte(elemHash),
		byte(elemHash >> 8),
		byte(elemHash >> 16),
		byte(elemHash >> 24),
	}); err != nil {
		panic(err)
	}
}

func (a *OrderedCollectionHasher) Sum() uint32 {
	return a.h.Sum32()
}

type UnorderedCollectionHasher struct {
	h uint32
}

func NewUnorderedCollectionHasher() UnorderedCollectionHasher {
	return UnorderedCollectionHasher{}
}

func (m *UnorderedCollectionHasher) WriteHash(elemHash uint32) {
	m.h ^= elemHash
}

func (m *UnorderedCollectionHasher) Sum() uint32 {
	h := NewHash()
	if _, err := h.Write([]byte{
		byte(m.h),
		byte(m.h >> 8),
		byte(m.h >> 16),
		byte(m.h >> 24),
	}); err != nil {
		panic(err)
	}
	return h.Sum32()
}

type Int_t int64
type Float_t float64
type UtfCodepoint_t rune
type String_t string
type Bool_t bool

func (i Int_t) Hash() uint32 {
	h := NewHash()
	if _, err := h.Write([]byte{
		byte(i),
		byte(i >> 8),
		byte(i >> 16),
		byte(i >> 24),
		byte(i >> 32),
		byte(i >> 40),
		byte(i >> 48),
		byte(i >> 56),
	}); err != nil {
		panic(err)
	}
	return h.Sum32()
}
func (i Int_t) Equal(o Int_t) bool { return i == o }

func (f Float_t) Hash() uint32 {
	h := NewHash()
	if err := binary.Write(h,
		binary.LittleEndian, f); err != nil {
		panic(err)
	}
	return h.Sum32()
}
func (f Float_t) Equal(o Float_t) bool { return f == o }

func (c UtfCodepoint_t) Hash() uint32 {
	h := NewHash()
	if _, err := h.Write([]byte{
		byte(c),
		byte(c >> 8),
		byte(c >> 16),
		byte(c >> 24),
	}); err != nil {
		panic(err)
	}
	return h.Sum32()
}
func (c UtfCodepoint_t) Equal(o UtfCodepoint_t) bool { return c == o }

func (s String_t) Hash() uint32 {
	h := NewHash()
	if _, err := h.Write([]byte(s)); err != nil {
		panic(err)
	}
	return h.Sum32()
}
func (s String_t) Equal(o String_t) bool { return s == o }

var trueHash = HashTuple(1)
var falseHash = HashTuple(0)

func (b Bool_t) Hash() uint32 {
	if b {
		return trueHash
	} else {
		return falseHash
	}
}
func (b Bool_t) Equal(o Bool_t) bool { return b == o }

type Tuple0_c struct{}
type Tuple0_t = Tuple0_c
type Tuple1_c[T0 Type[T0]] struct{ P_0 T0 }
type Tuple1_t[T0 Type[T0]] = Tuple1_c[T0]
type Tuple2_c[T0 Type[T0], T1 Type[T1]] struct {
	P_0 T0
	P_1 T1
}
type Tuple2_t[T0 Type[T0], T1 Type[T1]] = Tuple2_c[T0, T1]
type Tuple3_c[T0 Type[T0], T1 Type[T1], T2 Type[T2]] struct {
	P_0 T0
	P_1 T1
	P_2 T2
}
type Tuple3_t[T0 Type[T0], T1 Type[T1], T2 Type[T2]] = Tuple3_c[T0, T1, T2]
type Tuple4_c[T0 Type[T0], T1 Type[T1], T2 Type[T2], T3 Type[T3]] struct {
	P_0 T0
	P_1 T1
	P_2 T2
	P_3 T3
}
type Tuple4_t[T0 Type[T0], T1 Type[T1], T2 Type[T2], T3 Type[T3]] = Tuple4_c[T0, T1, T2, T3]
type Tuple5_c[T0 Type[T0], T1 Type[T1], T2 Type[T2], T3 Type[T3], T4 Type[T4]] struct {
	P_0 T0
	P_1 T1
	P_2 T2
	P_3 T3
	P_4 T4
}
type Tuple5_t[T0 Type[T0], T1 Type[T1], T2 Type[T2], T3 Type[T3], T4 Type[T4]] = Tuple5_c[T0, T1, T2, T3, T4]
type Tuple6_c[T0 Type[T0], T1 Type[T1], T2 Type[T2], T3 Type[T3], T4 Type[T4], T5 Type[T5]] struct {
	P_0 T0
	P_1 T1
	P_2 T2
	P_3 T3
	P_4 T4
	P_5 T5
}
type Tuple6_t[T0 Type[T0], T1 Type[T1], T2 Type[T2], T3 Type[T3], T4 Type[T4], T5 Type[T5]] = Tuple6_c[T0, T1, T2, T3, T4, T5]
type Tuple7_c[T0 Type[T0], T1 Type[T1], T2 Type[T2], T3 Type[T3], T4 Type[T4], T5 Type[T5], T6 Type[T6]] struct {
	P_0 T0
	P_1 T1
	P_2 T2
	P_3 T3
	P_4 T4
	P_5 T5
	P_6 T6
}
type Tuple7_t[T0 Type[T0], T1 Type[T1], T2 Type[T2], T3 Type[T3], T4 Type[T4], T5 Type[T5], T6 Type[T6]] = Tuple7_c[T0, T1, T2, T3, T4, T5, T6]
type Tuple8_c[T0 Type[T0], T1 Type[T1], T2 Type[T2], T3 Type[T3], T4 Type[T4], T5 Type[T5], T6 Type[T6], T7 Type[T7]] struct {
	P_0 T0
	P_1 T1
	P_2 T2
	P_3 T3
	P_4 T4
	P_5 T5
	P_6 T6
	P_7 T7
}
type Tuple8_t[T0 Type[T0], T1 Type[T1], T2 Type[T2], T3 Type[T3], T4 Type[T4], T5 Type[T5], T6 Type[T6], T7 Type[T7]] = Tuple8_c[T0, T1, T2, T3, T4, T5, T6, T7]
type Tuple9_c[T0 Type[T0], T1 Type[T1], T2 Type[T2], T3 Type[T3], T4 Type[T4], T5 Type[T5], T6 Type[T6], T7 Type[T7], T8 Type[T8]] struct {
	P_0 T0
	P_1 T1
	P_2 T2
	P_3 T3
	P_4 T4
	P_5 T5
	P_6 T6
	P_7 T7
	P_8 T8
}
type Tuple9_t[T0 Type[T0], T1 Type[T1], T2 Type[T2], T3 Type[T3], T4 Type[T4], T5 Type[T5], T6 Type[T6], T7 Type[T7], T8 Type[T8]] = Tuple9_c[T0, T1, T2, T3, T4, T5, T6, T7, T8]

type Indexable interface{ GetAt(i any) (any, bool) }

type Tuple_dyn interface {
	ToDynamic() []Dynamic_t
	Indexable
}

var NilHash = HashTuple()

func (Tuple0_c) ToDynamic() []Dynamic_t  { return []Dynamic_t{} }
func (Tuple0_c) GetAt(i any) (any, bool) { return nil, false }
func (Tuple0_c) Hash() uint32            { return NilHash }
func (Tuple0_c) Equal(Tuple0_c) bool     { return true }

func (t Tuple1_c[T0]) ToDynamic() []Dynamic_t {
	return []Dynamic_t{{t.P_0}}
}
func (t Tuple1_c[T0]) GetAt(i any) (any, bool) {
	switch i {
	case int64(0):
		return t.P_0, true
	}
	return nil, false
}
func (t Tuple1_c[T0]) Hash() uint32              { return t.P_0.Hash() }
func (t Tuple1_c[T0]) Equal(o Tuple1_c[T0]) bool { return t.P_0.Equal(o.P_0) }

func (t Tuple2_c[T0, T1]) ToDynamic() []Dynamic_t {
	return []Dynamic_t{{t.P_0}, {t.P_1}}
}
func (t Tuple2_c[T0, T1]) GetAt(i any) (any, bool) {
	switch i {
	case int64(0):
		return t.P_0, true
	case int64(1):
		return t.P_1, true
	}
	return nil, false
}
func (t Tuple2_c[T0, T1]) Hash() uint32 {
	return HashTuple(t.P_0.Hash(), t.P_1.Hash())
}
func (t Tuple2_c[T0, T1]) Equal(o Tuple2_c[T0, T1]) bool {
	return t.P_0.Equal(o.P_0) && t.P_1.Equal(o.P_1)
}

func (t Tuple3_c[T0, T1, T2]) ToDynamic() []Dynamic_t {
	return []Dynamic_t{{t.P_0}, {t.P_1}, {t.P_2}}
}
func (t Tuple3_c[T0, T1, T2]) GetAt(i any) (any, bool) {
	switch i {
	case int64(0):
		return t.P_0, true
	case int64(1):
		return t.P_1, true
	case int64(2):
		return t.P_2, true
	}
	return nil, false
}
func (t Tuple3_c[T0, T1, T2]) Hash() uint32 {
	return HashTuple(t.P_0.Hash(), t.P_1.Hash(), t.P_2.Hash())
}
func (t Tuple3_c[T0, T1, T2]) Equal(o Tuple3_c[T0, T1, T2]) bool {
	return t.P_0.Equal(o.P_0) && t.P_1.Equal(o.P_1) && t.P_2.Equal(o.P_2)
}

func (t Tuple4_c[T0, T1, T2, T3]) ToDynamic() []Dynamic_t {
	return []Dynamic_t{{t.P_0}, {t.P_1}, {t.P_2}, {t.P_3}}
}
func (t Tuple4_c[T0, T1, T2, T3]) GetAt(i any) (any, bool) {
	switch i {
	case int64(0):
		return t.P_0, true
	case int64(1):
		return t.P_1, true
	case int64(2):
		return t.P_2, true
	case int64(3):
		return t.P_3, true
	}
	return nil, false
}
func (t Tuple4_c[T0, T1, T2, T3]) Hash() uint32 {
	return HashTuple(t.P_0.Hash(), t.P_1.Hash(), t.P_2.Hash(), t.P_3.Hash())
}
func (t Tuple4_c[T0, T1, T2, T3]) Equal(o Tuple4_c[T0, T1, T2, T3]) bool {
	return t.P_0.Equal(o.P_0) && t.P_1.Equal(o.P_1) && t.P_2.Equal(o.P_2) && t.P_3.Equal(o.P_3)
}

func (t Tuple5_c[T0, T1, T2, T3, T4]) ToDynamic() []Dynamic_t {
	return []Dynamic_t{{t.P_0}, {t.P_1}, {t.P_2}, {t.P_3}, {t.P_4}}
}
func (t Tuple5_c[T0, T1, T2, T3, T4]) GetAt(i any) (any, bool) {
	switch i {
	case int64(0):
		return t.P_0, true
	case int64(1):
		return t.P_1, true
	case int64(2):
		return t.P_2, true
	case int64(3):
		return t.P_3, true
	case int64(4):
		return t.P_4, true
	}
	return nil, false
}
func (t Tuple5_c[T0, T1, T2, T3, T4]) Hash() uint32 {
	return HashTuple(t.P_0.Hash(), t.P_1.Hash(), t.P_2.Hash(), t.P_3.Hash(), t.P_4.Hash())
}
func (t Tuple5_c[T0, T1, T2, T3, T4]) Equal(o Tuple5_c[T0, T1, T2, T3, T4]) bool {
	return t.P_0.Equal(o.P_0) && t.P_1.Equal(o.P_1) && t.P_2.Equal(o.P_2) && t.P_3.Equal(o.P_3) && t.P_4.Equal(o.P_4)
}

func (t Tuple6_c[T0, T1, T2, T3, T4, T5]) ToDynamic() []Dynamic_t {
	return []Dynamic_t{{t.P_0}, {t.P_1}, {t.P_2}, {t.P_3}, {t.P_4}, {t.P_5}}
}
func (t Tuple6_c[T0, T1, T2, T3, T4, T5]) GetAt(i any) (any, bool) {
	switch i {
	case int64(0):
		return t.P_0, true
	case int64(1):
		return t.P_1, true
	case int64(2):
		return t.P_2, true
	case int64(3):
		return t.P_3, true
	case int64(4):
		return t.P_4, true
	case int64(5):
		return t.P_5, true
	}
	return nil, false
}
func (t Tuple6_c[T0, T1, T2, T3, T4, T5]) Hash() uint32 {
	return HashTuple(t.P_0.Hash(), t.P_1.Hash(), t.P_2.Hash(), t.P_3.Hash(), t.P_4.Hash(), t.P_5.Hash())
}
func (t Tuple6_c[T0, T1, T2, T3, T4, T5]) Equal(o Tuple6_c[T0, T1, T2, T3, T4, T5]) bool {
	return t.P_0.Equal(o.P_0) && t.P_1.Equal(o.P_1) && t.P_2.Equal(o.P_2) && t.P_3.Equal(o.P_3) && t.P_4.Equal(o.P_4) && t.P_5.Equal(o.P_5)
}

func (t Tuple7_c[T0, T1, T2, T3, T4, T5, T6]) ToDynamic() []Dynamic_t {
	return []Dynamic_t{{t.P_0}, {t.P_1}, {t.P_2}, {t.P_3}, {t.P_4}, {t.P_5}, {t.P_6}}
}
func (t Tuple7_c[T0, T1, T2, T3, T4, T5, T6]) GetAt(i any) (any, bool) {
	switch i {
	case int64(0):
		return t.P_0, true
	case int64(1):
		return t.P_1, true
	case int64(2):
		return t.P_2, true
	case int64(3):
		return t.P_3, true
	case int64(4):
		return t.P_4, true
	case int64(5):
		return t.P_5, true
	case int64(6):
		return t.P_6, true
	}
	return nil, false
}
func (t Tuple7_c[T0, T1, T2, T3, T4, T5, T6]) Hash() uint32 {
	return HashTuple(t.P_0.Hash(), t.P_1.Hash(), t.P_2.Hash(), t.P_3.Hash(), t.P_4.Hash(), t.P_5.Hash(), t.P_6.Hash())
}
func (t Tuple7_c[T0, T1, T2, T3, T4, T5, T6]) Equal(o Tuple7_c[T0, T1, T2, T3, T4, T5, T6]) bool {
	return t.P_0.Equal(o.P_0) && t.P_1.Equal(o.P_1) && t.P_2.Equal(o.P_2) && t.P_3.Equal(o.P_3) && t.P_4.Equal(o.P_4) && t.P_5.Equal(o.P_5) && t.P_6.Equal(o.P_6)
}

func (t Tuple8_c[T0, T1, T2, T3, T4, T5, T6, T7]) ToDynamic() []Dynamic_t {
	return []Dynamic_t{{t.P_0}, {t.P_1}, {t.P_2}, {t.P_3}, {t.P_4}, {t.P_5}, {t.P_6}, {t.P_7}}
}
func (t Tuple8_c[T0, T1, T2, T3, T4, T5, T6, T7]) GetAt(i any) (any, bool) {
	switch i {
	case int64(0):
		return t.P_0, true
	case int64(1):
		return t.P_1, true
	case int64(2):
		return t.P_2, true
	case int64(3):
		return t.P_3, true
	case int64(4):
		return t.P_4, true
	case int64(5):
		return t.P_5, true
	case int64(6):
		return t.P_6, true
	case int64(7):
		return t.P_7, true
	}
	return nil, false
}
func (t Tuple8_c[T0, T1, T2, T3, T4, T5, T6, T7]) Hash() uint32 {
	return HashTuple(t.P_0.Hash(), t.P_1.Hash(), t.P_2.Hash(), t.P_3.Hash(), t.P_4.Hash(), t.P_5.Hash(), t.P_6.Hash(), t.P_7.Hash())
}
func (t Tuple8_c[T0, T1, T2, T3, T4, T5, T6, T7]) Equal(o Tuple8_c[T0, T1, T2, T3, T4, T5, T6, T7]) bool {
	return t.P_0.Equal(o.P_0) && t.P_1.Equal(o.P_1) && t.P_2.Equal(o.P_2) && t.P_3.Equal(o.P_3) && t.P_4.Equal(o.P_4) && t.P_5.Equal(o.P_5) && t.P_6.Equal(o.P_6) && t.P_7.Equal(o.P_7)
}

func (t Tuple9_c[T0, T1, T2, T3, T4, T5, T6, T7, T8]) ToDynamic() []Dynamic_t {
	return []Dynamic_t{{t.P_0}, {t.P_1}, {t.P_2}, {t.P_3}, {t.P_4}, {t.P_5}, {t.P_6}, {t.P_7}, {t.P_8}}
}
func (t Tuple9_c[T0, T1, T2, T3, T4, T5, T6, T7, T8]) GetAt(i any) (any, bool) {
	switch i {
	case int64(0):
		return t.P_0, true
	case int64(1):
		return t.P_1, true
	case int64(2):
		return t.P_2, true
	case int64(3):
		return t.P_3, true
	case int64(4):
		return t.P_4, true
	case int64(5):
		return t.P_5, true
	case int64(6):
		return t.P_6, true
	case int64(7):
		return t.P_7, true
	case int64(8):
		return t.P_8, true
	}
	return nil, false
}
func (t Tuple9_c[T0, T1, T2, T3, T4, T5, T6, T7, T8]) Hash() uint32 {
	return HashTuple(t.P_0.Hash(), t.P_1.Hash(), t.P_2.Hash(), t.P_3.Hash(), t.P_4.Hash(), t.P_5.Hash(), t.P_6.Hash(), t.P_7.Hash(), t.P_8.Hash())
}
func (t Tuple9_c[T0, T1, T2, T3, T4, T5, T6, T7, T8]) Equal(o Tuple9_c[T0, T1, T2, T3, T4, T5, T6, T7, T8]) bool {
	return t.P_0.Equal(o.P_0) && t.P_1.Equal(o.P_1) && t.P_2.Equal(o.P_2) && t.P_3.Equal(o.P_3) && t.P_4.Equal(o.P_4) && t.P_5.Equal(o.P_5) && t.P_6.Equal(o.P_6) && t.P_7.Equal(o.P_7) && t.P_8.Equal(o.P_8)
}

type Func0_t[R Type[R]] func() R
type Func1_t[A0 Type[A0], R Type[R]] func(A0) R
type Func2_t[A0 Type[A0], A1 Type[A1], R Type[R]] func(A0, A1) R
type Func3_t[A0 Type[A0], A1 Type[A1], A2 Type[A2], R Type[R]] func(A0, A1, A2) R
type Func4_t[A0 Type[A0], A1 Type[A1], A2 Type[A2], A3 Type[A3], R Type[R]] func(A0, A1, A2, A3) R
type Func5_t[A0 Type[A0], A1 Type[A1], A2 Type[A2], A3 Type[A3], A4 Type[A4], R Type[R]] func(A0, A1, A2, A3, A4) R
type Func6_t[A0 Type[A0], A1 Type[A1], A2 Type[A2], A3 Type[A3], A4 Type[A4], A5 Type[A5], R Type[R]] func(A0, A1, A2, A3, A4, A5) R
type Func7_t[A0 Type[A0], A1 Type[A1], A2 Type[A2], A3 Type[A3], A4 Type[A4], A5 Type[A5], A6 Type[A6], R Type[R]] func(A0, A1, A2, A3, A4, A5, A6) R
type Func8_t[A0 Type[A0], A1 Type[A1], A2 Type[A2], A3 Type[A3], A4 Type[A4], A5 Type[A5], A6 Type[A6], A7 Type[A7], R Type[R]] func(A0, A1, A2, A3, A4, A5, A6, A7) R
type Func9_t[A0 Type[A0], A1 Type[A1], A2 Type[A2], A3 Type[A3], A4 Type[A4], A5 Type[A5], A6 Type[A6], A7 Type[A7], A8 Type[A8], R Type[R]] func(A0, A1, A2, A3, A4, A5, A6, A7, A8) R

func (Func0_t[_]) Hash() uint32                                                  { return NilHash }
func (Func0_t[R]) Equal(Func0_t[R]) bool                                         { return false }
func (Func1_t[_, _]) Hash() uint32                                               { return NilHash }
func (Func1_t[A0, R]) Equal(Func1_t[A0, R]) bool                                 { return false }
func (Func2_t[_, _, _]) Hash() uint32                                            { return NilHash }
func (Func2_t[A0, A1, R]) Equal(Func2_t[A0, A1, R]) bool                         { return false }
func (Func3_t[_, _, _, _]) Hash() uint32                                         { return NilHash }
func (Func3_t[A0, A1, A2, R]) Equal(Func3_t[A0, A1, A2, R]) bool                 { return false }
func (Func4_t[_, _, _, _, _]) Hash() uint32                                      { return NilHash }
func (Func4_t[A0, A1, A2, A3, R]) Equal(Func4_t[A0, A1, A2, A3, R]) bool         { return false }
func (Func5_t[_, _, _, _, _, _]) Hash() uint32                                   { return NilHash }
func (Func5_t[A0, A1, A2, A3, A4, R]) Equal(Func5_t[A0, A1, A2, A3, A4, R]) bool { return false }
func (Func6_t[_, _, _, _, _, _, _]) Hash() uint32                                { return NilHash }
func (Func6_t[A0, A1, A2, A3, A4, A5, R]) Equal(Func6_t[A0, A1, A2, A3, A4, A5, R]) bool {
	return false
}
func (Func7_t[_, _, _, _, _, _, _, _]) Hash() uint32 { return NilHash }
func (Func7_t[A0, A1, A2, A3, A4, A5, A6, R]) Equal(Func7_t[A0, A1, A2, A3, A4, A5, A6, R]) bool {
	return false
}
func (Func8_t[_, _, _, _, _, _, _, _, _]) Hash() uint32 { return NilHash }
func (Func8_t[A0, A1, A2, A3, A4, A5, A6, A7, R]) Equal(Func8_t[A0, A1, A2, A3, A4, A5, A6, A7, R]) bool {
	return false
}
func (Func9_t[_, _, _, _, _, _, _, _, _, _]) Hash() uint32 { return NilHash }
func (Func9_t[A0, A1, A2, A3, A4, A5, A6, A7, A8, R]) Equal(Func9_t[A0, A1, A2, A3, A4, A5, A6, A7, A8, R]) bool {
	return false
}

type Nil_c struct{}
type Nil_t = Nil_c

func (Nil_c) Hash() uint32     { return NilHash }
func (Nil_c) Equal(Nil_c) bool { return true }

type Result_t[T Type[T], E Type[E]] interface {
	iResult_t(T, E)
	IsOk() Bool_t
	AsOk() Ok_c[T, E]
	IsError() Bool_t
	AsError() Error_c[T, E]
	Result_dyn
	Type[Result_t[T, E]]
}

type Result_dyn interface {
	ToDynamic() (ok Dynamic_t, err Dynamic_t, isOk bool)
}

type Ok_c[T Type[T], E Type[E]] struct {
	P_0 T
}

func (Ok_c[T, E]) iResult_t(T, E)           {}
func (Ok_c[T, E]) IsOk() Bool_t             { return true }
func (c Ok_c[T, E]) AsOk() Ok_c[T, E]       { return c }
func (Ok_c[T, E]) IsError() Bool_t          { return false }
func (c Ok_c[T, E]) AsError() Error_c[T, E] { panic("expected Error value") }
func (c Ok_c[T, E]) ToDynamic() (ok Dynamic_t, err Dynamic_t, isOk bool) {
	return Dynamic_t{c.P_0}, Dynamic_t{}, true
}
func (c Ok_c[T, E]) Hash() uint32 { return HashConstructor(0, c.P_0.Hash()) }
func (c Ok_c[T, E]) Equal(o Result_t[T, E]) bool {
	if o, ok := o.(Ok_c[T, E]); ok {
		return c.P_0.Equal(o.P_0)
	}
	return false
}

var _ Result_dyn = Ok_c[Nil_t, Nil_t]{}

type Error_c[T Type[T], E Type[E]] struct {
	P_0 E
}

func (Error_c[T, E]) iResult_t(T, E)           {}
func (Error_c[T, E]) IsOk() Bool_t             { return false }
func (Error_c[T, E]) AsOk() Ok_c[T, E]         { panic("expected Ok value") }
func (Error_c[T, E]) IsError() Bool_t          { return true }
func (c Error_c[T, E]) AsError() Error_c[T, E] { return c }
func (c Error_c[T, E]) ToDynamic() (ok Dynamic_t, err Dynamic_t, isOk bool) {
	return Dynamic_t{}, Dynamic_t{c.P_0}, false
}
func (c Error_c[T, E]) Hash() uint32 { return HashConstructor(1, c.P_0.Hash()) }
func (c Error_c[T, E]) Equal(o Result_t[T, E]) bool {
	if o, ok := o.(Error_c[T, E]); ok {
		return c.P_0.Equal(o.P_0)
	}
	return false
}

var _ Result_dyn = Error_c[Nil_t, Nil_t]{}

type List_t[T Type[T]] interface {
	iList_t(T)
	HasLength(int) Bool_t
	AtLeastLength(int) Bool_t
	Head() T
	Tail() List_t[T]
	List_dyn
	Type[List_t[T]]
}

type List_dyn interface {
	ToDynamic() List_t[Dynamic_t]
	Indexable
}

type Empty_c[T Type[T]] struct{}

func (Empty_c[T]) iList_t(T)                    {}
func (Empty_c[T]) HasLength(n int) Bool_t       { return n == 0 }
func (Empty_c[T]) AtLeastLength(n int) Bool_t   { return n <= 0 }
func (Empty_c[T]) Head() T                      { panic("Empty list") }
func (Empty_c[T]) Tail() List_t[T]              { panic("Empty list") }
func (Empty_c[T]) ToDynamic() List_t[Dynamic_t] { return Empty_c[Dynamic_t]{} }
func (Empty_c[T]) GetAt(i any) (any, bool)      { return nil, false }
func (Empty_c[T]) Hash() uint32                 { return HashConstructor(0) }
func (Empty_c[T]) Equal(o List_t[T]) bool       { return o == Empty_c[T]{} }

var _ List_dyn = Empty_c[Nil_t]{}

type Nonempty_c[T Type[T]] struct {
	P_0 T
	P_1 List_t[T]
}

func (Nonempty_c[T]) iList_t(T)                    {}
func (l Nonempty_c[T]) HasLength(n int) Bool_t     { return n > 0 && l.P_1.HasLength(n-1) }
func (l Nonempty_c[T]) AtLeastLength(n int) Bool_t { return n <= 1 || l.P_1.AtLeastLength(n-1) }
func (l Nonempty_c[T]) Head() T                    { return l.P_0 }
func (l Nonempty_c[T]) Tail() List_t[T]            { return l.P_1 }
func (l Nonempty_c[T]) ToDynamic() List_t[Dynamic_t] {
	return Nonempty_c[Dynamic_t]{Dynamic_t{l.P_0}, l.P_1.ToDynamic()}
}
func (l Nonempty_c[T]) GetAt(i any) (any, bool) {
	if i, ok := i.(int64); ok && i >= 0 && i <= 2 {
		if i == 0 {
			return l.P_0, true
		} else {
			return l.P_1.GetAt(i - 1)
		}
	}
	return nil, false
}
func (l Nonempty_c[T]) Hash() uint32 {
	return HashConstructor(1, l.P_0.Hash(), l.P_1.Hash())
}
func (l Nonempty_c[T]) Equal(o List_t[T]) bool {
	if o, ok := o.(Nonempty_c[T]); ok {
		return l.P_0.Equal(o.P_0) && l.P_1.Equal(o.P_1)
	}
	return false
}

var _ List_dyn = Nonempty_c[Nil_t]{}

func ToList[T Type[T]](xs ...T) List_t[T] {
	if len(xs) == 0 {
		return Empty_c[T]{}
	}
	return Nonempty_c[T]{P_0: xs[0], P_1: ToList(xs[1:]...)}
}

func ListPrepend[T Type[T]](x T, xs List_t[T]) List_t[T] {
	return Nonempty_c[T]{P_0: x, P_1: xs}
}

type BitArray_t []byte

func (b BitArray_t) Hash() uint32 {
	h := NewHash()
	if _, err := h.Write(b); err != nil {
		panic(err)
	}
	return h.Sum32()
}
func (b BitArray_t) Equal(o BitArray_t) bool { return bytes.Equal(b, o) }

func ToBitArray(segments ...[]byte) BitArray_t {
	res := []byte{}
	for _, seg := range segments {
		res = append(res, seg...)
	}
	return res
}

func (b BitArray_t) Buffer() []byte {
	return b
}

func (b BitArray_t) ByteAt(i int) int64 {
	if i >= len(b) {
		return -1
	}
	return int64(b[i])
}

func SizedInt(value int64, size int64, isBigEndian Bool_t) []byte {
	bytes := make([]byte, size/8)
	if size == 32 {
		bits := uint32(value)
		if hostIsBigEndian == bool(isBigEndian) {
			binary.LittleEndian.PutUint32(bytes, bits)
		} else {
			binary.BigEndian.PutUint32(bytes, bits)
		}
	} else if size == 64 {
		bits := uint64(value)
		if hostIsBigEndian == bool(isBigEndian) {
			binary.LittleEndian.PutUint64(bytes, bits)
		} else {
			binary.BigEndian.PutUint64(bytes, bits)
		}
	} else {
		panic(fmt.Sprintf("Sized floats must be 32-bit or 64-bit on Go, got size of %d bits", size))
	}
	return bytes
}

func SizedFloat(value Float_t, size Int_t, isBigEndian Bool_t) []byte {
	bytes := make([]byte, size/8)
	if size == 32 {
		bits := math.Float32bits(float32(value))
		if hostIsBigEndian == bool(isBigEndian) {
			binary.LittleEndian.PutUint32(bytes, bits)
		} else {
			binary.BigEndian.PutUint32(bytes, bits)
		}
	} else if size == 64 {
		bits := math.Float64bits(float64(value))
		if hostIsBigEndian == bool(isBigEndian) {
			binary.LittleEndian.PutUint64(bytes, bits)
		} else {
			binary.BigEndian.PutUint64(bytes, bits)
		}
	} else {
		panic(fmt.Sprintf("Sized floats must be 32-bit or 64-bit on Go, got size of %d bits", size))
	}
	return bytes
}

func StringBits(s string) []byte {
	return []byte(s)
}

func (b BitArray_t) IntFromSlice(start, end Int_t, isBigEndian, isSigned Bool_t) Int_t {
	return byteArrayToInt(b, start, end, isBigEndian, isSigned)
}

func (b BitArray_t) FloatFromSlice(start, end Int_t, isBigEndian Bool_t) Float_t {
	return byteArrayToFloat(b, start, end, isBigEndian)
}

func (b BitArray_t) SliceAfter(start Int_t) BitArray_t {
	return b[start:]
}

func (b BitArray_t) BinaryFromSlice(start, end Int_t) BitArray_t {
	return b[start:end]
}

func byteArrayToInt(byteArray BitArray_t, start Int_t, end Int_t, isBigEndian Bool_t, isSigned Bool_t) Int_t {
	byteSize := end - start

	value := Int_t(0)

	// Read bytes as an unsigned integer value
	if isBigEndian {
		for i := start; i < end; i++ {
			value = value*256 + Int_t(byteArray[i])
		}
	} else {
		for i := end - 1; i >= start; i-- {
			value = value*256 + Int_t(byteArray[i])
		}
	}

	// For signed integers, check if the high bit is set and if so then
	// reinterpret as two's complement
	if isSigned {
		highBit := Int_t(2) << (byteSize*8 - 1)
		if value >= highBit {
			value -= highBit * 2
		}
	}

	return value
}

func byteArrayToFloat(byteArray BitArray_t, start, end Int_t, isBigEndian Bool_t) Float_t {
	byteSize := end - start

	if byteSize == 8 {
		bytes := byteArray[start:end]
		if hostIsBigEndian == bool(isBigEndian) {
			return Float_t(math.Float64frombits(binary.LittleEndian.Uint64(bytes)))
		} else {
			return Float_t(math.Float64frombits(binary.BigEndian.Uint64(bytes)))
		}
	} else if byteSize == 4 {
		bytes := byteArray[start:end]
		if hostIsBigEndian == bool(isBigEndian) {
			return Float_t(math.Float32frombits(binary.LittleEndian.Uint32(bytes)))
		} else {
			return Float_t(math.Float32frombits(binary.BigEndian.Uint32(bytes)))
		}
	} else {
		panic(fmt.Sprintf("Sized floats must be 32-bit or 64-bit on Go, got size of %d bits", byteSize*8))
	}
}

func CodepointBits(codepoint rune) []byte {
	return []byte(string(codepoint))
}

func DivideInt(a Int_t, b Int_t) Int_t {
	if b == 0 {
		return 0
	}
	return a / b
}

func RemainderInt(a Int_t, b Int_t) Int_t {
	if b == 0 {
		return 0
	}
	return a % b
}

func DivideFloat(a Float_t, b Float_t) Float_t {
	if b == 0.0 {
		return 0.0
	}
	return a / b
}

func MakeError(variant string, module string, line int, fn string, message string, extra any) error {
	return fmt.Errorf("%s: %s:%d:%s: %s (%#v)", variant, module, line, fn, message, extra)
}
