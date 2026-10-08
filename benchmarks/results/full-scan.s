__RNvCsaCpq0navtZb_9tokenizer4scan:
	.cfi_startproc
	sub	sp, sp, #144
	.cfi_def_cfa_offset 144
	stp	x28, x27, [sp, #48]
	stp	x26, x25, [sp, #64]
	stp	x24, x23, [sp, #80]
	stp	x22, x21, [sp, #96]
	stp	x20, x19, [sp, #112]
	stp	x29, x30, [sp, #128]
	add	x29, sp, #128
	.cfi_def_cfa w29, 16
	.cfi_offset w30, -8
	.cfi_offset w29, -16
	.cfi_offset w19, -24
	.cfi_offset w20, -32
	.cfi_offset w21, -40
	.cfi_offset w22, -48
	.cfi_offset w23, -56
	.cfi_offset w24, -64
	.cfi_offset w25, -72
	.cfi_offset w26, -80
	.cfi_offset w27, -88
	.cfi_offset w28, -96
	stp	x0, x1, [sp]
	str	xzr, [sp, #16]
	cbz	x1, LBB17_77
	mov	x20, #0
	mov	x19, #0
Lloh152:
	adrp	x21, l_anon.f14e5187b3c17eb17ee7d0d31484fae9.51@PAGE
Lloh153:
	add	x21, x21, l_anon.f14e5187b3c17eb17ee7d0d31484fae9.51@PAGEOFF
	mov	x22, #7
	movk	x22, #3, lsl #32
	mov	w23, #3
	add	x24, sp, #24
	mov	w25, #5
Lloh154:
	adrp	x26, LJTI17_0@PAGE
Lloh155:
	add	x26, x26, LJTI17_0@PAGEOFF
LBB17_2:
	ldr	x8, [sp]
	b	LBB17_4
LBB17_3:
	mov	w9, #1
	add	x20, x20, x9
	cmp	x20, x1
	str	x20, [sp, #16]
	b.hs	LBB17_78
LBB17_4:
	add	x9, x8, x20
	ldrb	w27, [x9]
	ldrb	w10, [x21, w27, uxtw]
	tbnz	w10, #0, LBB17_3
	tbz	w27, #7, LBB17_23
	sub	x10, x1, x20
	cmp	x10, #3
	b.lo	LBB17_23
	cmp	w27, #227
	b.eq	LBB17_14
	cmp	w27, #226
	b.eq	LBB17_11
	cmp	w27, #225
	b.ne	LBB17_23
	ldrb	w10, [x9, #1]
	cmp	w10, #154
	b.eq	LBB17_15
	b	LBB17_20
LBB17_11:
	ldrb	w10, [x9, #1]
	cmp	w10, #128
	b.eq	LBB17_17
	cmp	w10, #129
	b.ne	LBB17_20
	ldrb	w9, [x9, #2]
	cmp	w9, #159
	b.eq	LBB17_16
	b	LBB17_20
LBB17_14:
	ldrb	w10, [x9, #1]
	cmp	w10, #128
	b.ne	LBB17_20
LBB17_15:
	ldrb	w9, [x9, #2]
	cmp	w9, #128
	b.ne	LBB17_20
LBB17_16:
	mov	w9, #3
	add	x20, x20, x9
	cmp	x20, x1
	str	x20, [sp, #16]
	b.lo	LBB17_4
	b	LBB17_78
LBB17_17:
	ldrsb	w9, [x9, #2]
	cmn	w9, #121
	b.lt	LBB17_16
	add	w9, w9, #120
	cmp	w9, #33
	b.hi	LBB17_20
	lsr	x9, x22, x9
	tbnz	w9, #0, LBB17_16
LBB17_20:
	add	x8, x20, #1
	str	x8, [sp, #16]
LBB17_21:
	mov	x0, sp
	mov	x1, x20
	bl	__RNvMCsj6EUeDfLBbc_10light_cljsNtB2_9Tokenizer8digit_at
	tbz	w0, #0, LBB17_42
LBB17_22:
	mov	w8, #5
	ldp	x9, x10, [sp, #8]
	subs	x9, x9, x10
	b.ls	LBB17_63
	b	LBB17_47
LBB17_23:
	add	x10, x20, #1
	str	x10, [sp, #16]
	sub	w11, w27, #34
	cmp	w11, #92
	b.hi	LBB17_40
	mov	w9, #5
Ltmp237:
	adr	x12, Ltmp237
	ldrsw	x13, [x26, x11, lsl #2]
	add	x12, x12, x13
	br	x12
LBB17_25:
	cmp	x10, x1
	b.lo	LBB17_28
	mov	w9, #0
	b	LBB17_64
LBB17_27:
	add	x9, x10, #2
	cmp	x11, x1
	csel	x10, x9, x11, lo
	cmp	x10, x1
	b.hs	LBB17_31
LBB17_28:
	ldrb	w9, [x8, x10]
	add	x11, x10, #1
	cmp	w9, #92
	b.eq	LBB17_27
	cmp	w9, #34
	b.eq	LBB17_76
	mov	x10, x11
	cmp	x11, x1
	b.lo	LBB17_28
LBB17_31:
	mov	w9, #0
	str	x10, [sp, #16]
	b	LBB17_64
LBB17_32:
	mov	w9, #10
	b	LBB17_64
LBB17_33:
	mov	w9, #21
	b	LBB17_64
LBB17_34:
	mov	w9, #9
	b	LBB17_64
LBB17_35:
	mov	x0, sp
	bl	__RNvMCsj6EUeDfLBbc_10light_cljsNtB2_9Tokenizer8dispatch
	and	w9, w0, #0xff
	b	LBB17_64
LBB17_36:
	cmp	x10, x1
	b.hs	LBB17_74
LBB17_37:
	ldrb	w9, [x8, x10]
	cmp	w9, #10
	ccmp	w9, #13, #4, ne
	b.eq	LBB17_74
	add	x10, x10, #1
	str	x10, [sp, #16]
	mov	w9, #15
	cmp	x1, x10
	b.ne	LBB17_37
	b	LBB17_64
LBB17_39:
	mov	w9, #17
	b	LBB17_64
LBB17_40:
	sub	w8, w27, #48
	cmp	w8, #10
	b.lo	LBB17_22
	tbnz	w27, #7, LBB17_21
LBB17_42:
	cmp	w27, #43
	b.eq	LBB17_44
	cmp	w27, #45
	b.ne	LBB17_46
LBB17_44:
	ldr	x1, [sp, #16]
	mov	x0, sp
	bl	__RNvMCsj6EUeDfLBbc_10light_cljsNtB2_9Tokenizer8digit_at
	.p2align	6, , 4
	cmp	w0, #0
	csel	w8, w25, w23, ne
	ldp	x9, x10, [sp, #8]
	subs	x9, x9, x10
	b.hi	LBB17_47
	b	LBB17_63
LBB17_46:
	mov	w8, #3
	ldp	x9, x10, [sp, #8]
	subs	x9, x9, x10
	b.ls	LBB17_63
LBB17_47:
	ldr	x11, [sp]
	add	x10, x10, #1
	add	x11, x11, #1
	b	LBB17_50
LBB17_48:
	ldrb	w12, [x11, x10]
	cmp	w12, #128
	b.eq	LBB17_63
LBB17_49:
	str	x10, [sp, #16]
	add	x10, x10, #1
	subs	x9, x9, #1
	b.eq	LBB17_63
LBB17_50:
	add	x12, x11, x10
	ldurb	w12, [x12, #-2]
	ldrb	w13, [x21, x12]
	tst	w13, w8
	b.ne	LBB17_63
	tbz	w12, #7, LBB17_49
	cmp	x9, #2
	b.ls	LBB17_49
	cmp	w12, #227
	b.eq	LBB17_60
	cmp	w12, #226
	b.eq	LBB17_57
	cmp	w12, #225
	b.ne	LBB17_49
	add	x12, x11, x10
	ldurb	w12, [x12, #-1]
	cmp	w12, #154
	b.ne	LBB17_49
	b	LBB17_48
LBB17_57:
	add	x12, x11, x10
	ldurb	w12, [x12, #-1]
	cmp	w12, #128
	b.eq	LBB17_61
	cmp	w12, #129
	b.ne	LBB17_49
	ldrb	w12, [x11, x10]
	cmp	w12, #159
	b.ne	LBB17_49
	b	LBB17_63
LBB17_60:
	add	x12, x11, x10
	ldurb	w12, [x12, #-1]
	cmp	w12, #128
	b.ne	LBB17_49
	b	LBB17_48
LBB17_61:
	ldrsb	w12, [x11, x10]
	cmn	w12, #121
	b.lt	LBB17_63
	add	w12, w12, #120
	cmp	w12, #34
	cset	w13, lo
	lsr	x12, x22, x12
	and	w12, w13, w12
	tbz	w12, #0, LBB17_49
LBB17_63:
	mov	w9, #11
LBB17_64:
	ldr	x8, [sp, #16]
	stp	x20, x8, [sp, #24]
	strb	w9, [sp, #40]
	; InlineAsm Start
	; InlineAsm End
	add	x19, x19, #1
	ldp	x1, x20, [sp, #8]
	cmp	x20, x1
	b.lo	LBB17_2
	b	LBB17_78
LBB17_65:
	mov	w9, #20
	b	LBB17_64
LBB17_66:
	mov	w9, #7
	b	LBB17_64
LBB17_67:
	cmp	x10, x1
	b.hs	LBB17_75
	ldrb	w8, [x8, x10]
	cmp	w8, #64
	b.ne	LBB17_75
	add	x8, x20, #2
	str	x8, [sp, #16]
	mov	w9, #19
	b	LBB17_64
LBB17_70:
	mov	w9, #16
	b	LBB17_64
LBB17_71:
	mov	w9, #6
	b	LBB17_64
LBB17_72:
	mov	w9, #8
	b	LBB17_64
LBB17_73:
	mov	x0, sp
	bl	__RNvMCsj6EUeDfLBbc_10light_cljsNtB2_9Tokenizer9character
	and	w9, w0, #0xff
	b	LBB17_64
LBB17_74:
	mov	w9, #15
	b	LBB17_64
LBB17_75:
	mov	w9, #18
	b	LBB17_64
LBB17_76:
	mov	w9, #12
	str	x11, [sp, #16]
	b	LBB17_64
LBB17_77:
	mov	x19, #0
LBB17_78:
	mov	x0, x19
	.cfi_def_cfa wsp, 144
	ldp	x29, x30, [sp, #128]
	ldp	x20, x19, [sp, #112]
	ldp	x22, x21, [sp, #96]
	ldp	x24, x23, [sp, #80]
	ldp	x26, x25, [sp, #64]
	ldp	x28, x27, [sp, #48]
	add	sp, sp, #144
	.cfi_def_cfa_offset 0
	.cfi_restore w30
	.cfi_restore w29
	.cfi_restore w19
	.cfi_restore w20
	.cfi_restore w21
	.cfi_restore w22
	.cfi_restore w23
	.cfi_restore w24
	.cfi_restore w25
	.cfi_restore w26
	.cfi_restore w27
	.cfi_restore w28
	ret
	.loh AdrpAdd	Lloh154, Lloh155
	.loh AdrpAdd	Lloh152, Lloh153
	.cfi_endproc
