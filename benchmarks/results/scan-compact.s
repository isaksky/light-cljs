__RNvCsaCpq0navtZb_9tokenizer12scan_compact:
	.cfi_startproc
	sub	sp, sp, #32
	.cfi_def_cfa_offset 32
	stp	x29, x30, [sp, #16]
	add	x29, sp, #16
	.cfi_def_cfa w29, 16
	.cfi_offset w30, -8
	.cfi_offset w29, -16
	.cfi_remember_state
	lsr	x8, x1, #32
	cbnz	x8, LBB20_20
	mov	x8, #0
	cbz	x1, LBB20_19
	mov	x11, #0
	mov	x9, sp
	mov	x10, #20895015895040
LBB20_3:
	mov	x12, x11
	b	LBB20_5
LBB20_4:
	add	x12, x12, #1
	cmp	x1, x12
	b.eq	LBB20_19
LBB20_5:
	ldrb	w13, [x0, x12]
	sub	w11, w13, #9
	cmp	w11, #5
	b.lo	LBB20_4
	cmp	w13, #44
	b.eq	LBB20_4
	cmp	w13, #32
	b.eq	LBB20_4
	add	x11, x12, #1
	cmp	w13, #41
	b.eq	LBB20_11
	cmp	w13, #40
	b.ne	LBB20_12
	mov	w13, #0
	stp	w12, w11, [sp]
	strb	w13, [sp, #8]
	; InlineAsm Start
	; InlineAsm End
	add	x8, x8, #1
	cmp	x11, x1
	b.lo	LBB20_3
	b	LBB20_19
LBB20_11:
	mov	w13, #1
	stp	w12, w11, [sp]
	strb	w13, [sp, #8]
	; InlineAsm Start
	; InlineAsm End
	add	x8, x8, #1
	cmp	x11, x1
	b.lo	LBB20_3
	b	LBB20_19
LBB20_12:
	cmp	x11, x1
	b.lo	LBB20_14
	b	LBB20_17
LBB20_13:
	add	x11, x11, #1
	mov	w13, #2
	cmp	x1, x11
	b.eq	LBB20_18
LBB20_14:
	ldrb	w13, [x0, x11]
	sub	w14, w13, #9
	cmp	w14, #5
	b.lo	LBB20_17
	cmp	w13, #44
	b.hi	LBB20_13
	lsr	x13, x10, x13
	tbz	w13, #0, LBB20_13
LBB20_17:
	mov	w13, #2
	stp	w12, w11, [sp]
	strb	w13, [sp, #8]
	; InlineAsm Start
	; InlineAsm End
	add	x8, x8, #1
	cmp	x11, x1
	b.lo	LBB20_3
	b	LBB20_19
LBB20_18:
	mov	x11, x1
	stp	w12, w11, [sp]
	strb	w13, [sp, #8]
	; InlineAsm Start
	; InlineAsm End
	add	x8, x8, #1
	cmp	x1, x1
	b.lo	LBB20_3
LBB20_19:
	mov	x0, x8
	.cfi_def_cfa wsp, 32
	ldp	x29, x30, [sp, #16]
	add	sp, sp, #32
	.cfi_def_cfa_offset 0
	.cfi_restore w30
	.cfi_restore w29
	ret
LBB20_20:
	.cfi_restore_state
Lloh14:
	adrp	x0, l_anon.f14e5187b3c17eb17ee7d0d31484fae9.7@PAGE
Lloh15:
	add	x0, x0, l_anon.f14e5187b3c17eb17ee7d0d31484fae9.7@PAGEOFF
Lloh16:
	adrp	x3, l_anon.f14e5187b3c17eb17ee7d0d31484fae9.52@PAGE
Lloh17:
	add	x3, x3, l_anon.f14e5187b3c17eb17ee7d0d31484fae9.52@PAGEOFF
Lloh18:
	adrp	x4, l_anon.f14e5187b3c17eb17ee7d0d31484fae9.8@PAGE
Lloh19:
	add	x4, x4, l_anon.f14e5187b3c17eb17ee7d0d31484fae9.8@PAGEOFF
	sub	x2, x29, #1
	mov	w1, #38
	bl	__RNvNtCs33v9mEcCS3W_4core6result13unwrap_failed
	.loh AdrpAdd	Lloh18, Lloh19
	.loh AdrpAdd	Lloh16, Lloh17
	.loh AdrpAdd	Lloh14, Lloh15
	.cfi_endproc
