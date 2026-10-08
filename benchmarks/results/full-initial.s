__RNvCsaCpq0navtZb_9tokenizer4scan:
	.cfi_startproc
	sub	sp, sp, #112
	.cfi_def_cfa_offset 112
	stp	x20, x19, [sp, #80]
	stp	x29, x30, [sp, #96]
	add	x29, sp, #96
	.cfi_def_cfa w29, 16
	.cfi_offset w30, -8
	.cfi_offset w29, -16
	.cfi_offset w19, -24
	.cfi_offset w20, -32
	stp	x0, x1, [sp]
	str	xzr, [sp, #16]
	add	x0, sp, #24
	mov	x1, sp
	bl	__RNvXs_Csj6EUeDfLBbc_10light_cljsNtB4_9TokenizerNtNtNtNtCs33v9mEcCS3W_4core4iter6traits8iterator8Iterator4next
	ldrb	w8, [sp, #40]
	cmp	w8, #255
	b.eq	LBB17_3
	mov	x19, #0
	add	x20, sp, #48
LBB17_2:
	ldur	q0, [sp, #24]
	str	q0, [sp, #48]
	ldr	x8, [sp, #40]
	str	x8, [sp, #64]
	; InlineAsm Start
	; InlineAsm End
	add	x19, x19, #1
	add	x0, sp, #24
	mov	x1, sp
	bl	__RNvXs_Csj6EUeDfLBbc_10light_cljsNtB4_9TokenizerNtNtNtNtCs33v9mEcCS3W_4core4iter6traits8iterator8Iterator4next
	ldrb	w8, [sp, #40]
	cmp	w8, #255
	b.ne	LBB17_2
	b	LBB17_4
LBB17_3:
	mov	x19, #0
LBB17_4:
	mov	x0, x19
	.cfi_def_cfa wsp, 112
	ldp	x29, x30, [sp, #96]
	ldp	x20, x19, [sp, #80]
	add	sp, sp, #112
	.cfi_def_cfa_offset 0
	.cfi_restore w30
	.cfi_restore w29
	.cfi_restore w19
	.cfi_restore w20
	ret
	.cfi_endproc

__RNvXs_Csj6EUeDfLBbc_10light_cljsNtB4_9TokenizerNtNtNtNtCs33v9mEcCS3W_4core4iter6traits8iterator8Iterator4next:
	.cfi_startproc
	stp	x24, x23, [sp, #-64]!
	.cfi_def_cfa_offset 64
	stp	x22, x21, [sp, #16]
	stp	x20, x19, [sp, #32]
	stp	x29, x30, [sp, #48]
	add	x29, sp, #48
	.cfi_def_cfa w29, 16
	.cfi_offset w30, -8
	.cfi_offset w29, -16
	.cfi_offset w19, -24
	.cfi_offset w20, -32
	.cfi_offset w21, -40
	.cfi_offset w22, -48
	.cfi_offset w23, -56
	.cfi_offset w24, -64
	.cfi_remember_state
	ldp	x8, x19, [x1, #8]
	cmp	x19, x8
	b.hs	LBB28_22
	ldr	x9, [x1]
Lloh172:
	adrp	x20, l_anon.f14e5187b3c17eb17ee7d0d31484fae9.51@PAGE
Lloh173:
	add	x20, x20, l_anon.f14e5187b3c17eb17ee7d0d31484fae9.51@PAGEOFF
	mov	x10, #7
	movk	x10, #3, lsl #32
	b	LBB28_3
LBB28_2:
	mov	w11, #1
	add	x19, x19, x11
	cmp	x19, x8
	str	x19, [x1, #16]
	b.hs	LBB28_22
LBB28_3:
	add	x11, x9, x19
	ldrb	w22, [x11]
	ldrb	w12, [x20, w22, uxtw]
	tbnz	w12, #0, LBB28_2
	tbz	w22, #7, LBB28_23
	sub	x12, x8, x19
	cmp	x12, #3
	b.lo	LBB28_23
	cmp	w22, #227
	b.eq	LBB28_13
	cmp	w22, #226
	b.eq	LBB28_10
	cmp	w22, #225
	b.ne	LBB28_23
	ldrb	w12, [x11, #1]
	cmp	w12, #154
	b.eq	LBB28_14
	b	LBB28_19
LBB28_10:
	ldrb	w12, [x11, #1]
	cmp	w12, #128
	b.eq	LBB28_16
	cmp	w12, #129
	b.ne	LBB28_19
	ldrb	w11, [x11, #2]
	cmp	w11, #159
	b.eq	LBB28_15
	b	LBB28_19
LBB28_13:
	ldrb	w12, [x11, #1]
	cmp	w12, #128
	b.ne	LBB28_19
LBB28_14:
	ldrb	w11, [x11, #2]
	cmp	w11, #128
	b.ne	LBB28_19
LBB28_15:
	mov	w11, #3
	add	x19, x19, x11
	cmp	x19, x8
	str	x19, [x1, #16]
	b.lo	LBB28_3
	b	LBB28_22
LBB28_16:
	ldrsb	w11, [x11, #2]
	cmn	w11, #121
	b.lt	LBB28_15
	add	w11, w11, #120
	cmp	w11, #33
	b.hi	LBB28_19
	lsr	x11, x10, x11
	tbnz	w11, #0, LBB28_15
LBB28_19:
	add	x8, x19, #1
	str	x8, [x1, #16]
LBB28_20:
	mov	x21, x0
	mov	x0, x1
	mov	x23, x1
	mov	x1, x19
	bl	__RNvMCsj6EUeDfLBbc_10light_cljsNtB2_9Tokenizer8digit_at
	mov	x1, x23
	mov	x8, x0
	mov	x0, x21
	tbz	w8, #0, LBB28_42
LBB28_21:
	mov	w21, #5
	b	LBB28_46
LBB28_22:
	mov	w10, #255
	b	LBB28_64
LBB28_23:
	add	x11, x19, #1
	str	x11, [x1, #16]
	sub	w12, w22, #34
	cmp	w12, #92
	b.hi	LBB28_40
	mov	w10, #5
Lloh174:
	adrp	x13, LJTI28_0@PAGE
Lloh175:
	add	x13, x13, LJTI28_0@PAGEOFF
	adr	x14, LBB28_25
	ldrb	w15, [x13, x12]
	add	x14, x14, x15, lsl #2
	br	x14
LBB28_25:
	cmp	x11, x8
	b.lo	LBB28_28
	mov	w10, #0
	b	LBB28_63
LBB28_27:
	add	x10, x11, #2
	cmp	x12, x8
	csel	x11, x10, x12, lo
	cmp	x11, x8
	b.hs	LBB28_31
LBB28_28:
	ldrb	w10, [x9, x11]
	add	x12, x11, #1
	cmp	w10, #92
	b.eq	LBB28_27
	cmp	w10, #34
	b.eq	LBB28_77
	mov	x11, x12
	cmp	x12, x8
	b.lo	LBB28_28
LBB28_31:
	mov	w10, #0
	str	x11, [x1, #16]
	b	LBB28_63
LBB28_32:
	mov	w10, #10
	b	LBB28_63
LBB28_33:
	mov	w10, #21
	b	LBB28_63
LBB28_34:
	mov	w10, #9
	b	LBB28_63
LBB28_35:
	mov	x20, x0
	mov	x0, x1
	mov	x21, x1
	bl	__RNvMCsj6EUeDfLBbc_10light_cljsNtB2_9Tokenizer8dispatch
	b	LBB28_74
LBB28_36:
	cmp	x11, x8
	b.hs	LBB28_75
LBB28_37:
	ldrb	w10, [x9, x11]
	cmp	w10, #10
	ccmp	w10, #13, #4, ne
	b.eq	LBB28_75
	add	x11, x11, #1
	str	x11, [x1, #16]
	mov	w10, #15
	cmp	x8, x11
	b.ne	LBB28_37
	b	LBB28_63
LBB28_39:
	mov	w10, #17
	b	LBB28_63
LBB28_40:
	sub	w8, w22, #48
	cmp	w8, #10
	b.lo	LBB28_21
	tbnz	w22, #7, LBB28_20
LBB28_42:
	mov	w21, #3
	cmp	w22, #43
	b.eq	LBB28_44
	cmp	w22, #45
	b.ne	LBB28_46
LBB28_44:
	ldr	x8, [x1, #16]
	mov	x22, x0
	mov	x0, x1
	mov	x23, x1
	mov	x1, x8
	bl	__RNvMCsj6EUeDfLBbc_10light_cljsNtB2_9Tokenizer8digit_at
	mov	x1, x23
	mov	x8, x0
	mov	x0, x22
	mov	w9, #5
	.p2align	6, , 4
	cmp	w8, #0
	csel	w21, w9, w21, ne
LBB28_46:
	ldp	x8, x9, [x1, #8]
	subs	x8, x8, x9
	b.ls	LBB28_62
	ldr	x10, [x1]
	add	x9, x9, #1
	add	x10, x10, #1
	mov	x11, #7
	movk	x11, #3, lsl #32
	b	LBB28_50
LBB28_48:
	ldrb	w12, [x10, x9]
	cmp	w12, #128
	b.eq	LBB28_62
LBB28_49:
	str	x9, [x1, #16]
	add	x9, x9, #1
	subs	x8, x8, #1
	b.eq	LBB28_62
LBB28_50:
	add	x12, x10, x9
	ldurb	w12, [x12, #-2]
	ldrb	w13, [x20, x12]
	tst	w13, w21
	b.ne	LBB28_62
	lsl	w13, w12, #24
	cmp	x8, #2
	ccmp	w13, #0, #0, hi
	b.pl	LBB28_49
	cmp	w12, #227
	b.eq	LBB28_59
	cmp	w12, #226
	b.eq	LBB28_56
	cmp	w12, #225
	b.ne	LBB28_49
	add	x12, x10, x9
	ldurb	w12, [x12, #-1]
	cmp	w12, #154
	b.ne	LBB28_49
	b	LBB28_48
LBB28_56:
	add	x12, x10, x9
	ldurb	w12, [x12, #-1]
	cmp	w12, #128
	b.eq	LBB28_60
	cmp	w12, #129
	b.ne	LBB28_49
	ldrb	w12, [x10, x9]
	cmp	w12, #159
	b.ne	LBB28_49
	b	LBB28_62
LBB28_59:
	add	x12, x10, x9
	ldurb	w12, [x12, #-1]
	cmp	w12, #128
	b.ne	LBB28_49
	b	LBB28_48
LBB28_60:
	ldrsb	w12, [x10, x9]
	cmn	w12, #121
	b.lt	LBB28_62
	add	w12, w12, #120
	cmp	w12, #34
	cset	w13, lo
	lsr	x12, x11, x12
	and	w12, w13, w12
	tbz	w12, #0, LBB28_49
LBB28_62:
	mov	w10, #11
LBB28_63:
	ldr	x8, [x1, #16]
	stp	x19, x8, [x0]
LBB28_64:
	strb	w10, [x0, #16]
	.cfi_def_cfa wsp, 64
	ldp	x29, x30, [sp, #48]
	ldp	x20, x19, [sp, #32]
	ldp	x22, x21, [sp, #16]
	ldp	x24, x23, [sp], #64
	.cfi_def_cfa_offset 0
	.cfi_restore w30
	.cfi_restore w29
	.cfi_restore w19
	.cfi_restore w20
	.cfi_restore w21
	.cfi_restore w22
	.cfi_restore w23
	.cfi_restore w24
	ret
LBB28_65:
	.cfi_restore_state
	mov	w10, #20
	b	LBB28_63
LBB28_66:
	mov	w10, #7
	b	LBB28_63
LBB28_67:
	cmp	x11, x8
	b.hs	LBB28_76
	ldrb	w8, [x9, x11]
	cmp	w8, #64
	b.ne	LBB28_76
	add	x8, x19, #2
	str	x8, [x1, #16]
	mov	w10, #19
	b	LBB28_63
LBB28_70:
	mov	w10, #16
	b	LBB28_63
LBB28_71:
	mov	w10, #6
	b	LBB28_63
LBB28_72:
	mov	w10, #8
	b	LBB28_63
LBB28_73:
	mov	x20, x0
	mov	x0, x1
	mov	x21, x1
	bl	__RNvMCsj6EUeDfLBbc_10light_cljsNtB2_9Tokenizer9character
LBB28_74:
	mov	x1, x21
	mov	x8, x0
	mov	x0, x20
	and	w10, w8, #0xff
	b	LBB28_63
LBB28_75:
	mov	w10, #15
	b	LBB28_63
LBB28_76:
	mov	w10, #18
	b	LBB28_63
LBB28_77:
	mov	w10, #12
	str	x12, [x1, #16]
	b	LBB28_63
	.loh AdrpAdd	Lloh172, Lloh173
	.loh AdrpAdd	Lloh174, Lloh175
	.cfi_endproc
