__RNvCsaCpq0navtZb_9tokenizer10scan_table:
	.cfi_startproc
	cbz	x1, LBB19_16
	sub	sp, sp, #32
	.cfi_def_cfa_offset 32
	mov	x8, x0
	mov	x0, #0
	mov	x11, #0
	add	x9, sp, #8
Lloh12:
	adrp	x10, l_anon.f14e5187b3c17eb17ee7d0d31484fae9.58@PAGE
Lloh13:
	add	x10, x10, l_anon.f14e5187b3c17eb17ee7d0d31484fae9.58@PAGEOFF
LBB19_2:
	mov	x12, x11
LBB19_3:
	ldrb	w13, [x8, x12]
	ldrb	w14, [x10, x13]
	add	x11, x12, #1
	tbz	w14, #0, LBB19_5
	mov	x12, x11
	cmp	x1, x11
	b.ne	LBB19_3
	b	LBB19_15
LBB19_5:
	cmp	w13, #41
	b.eq	LBB19_8
	cmp	w13, #40
	b.ne	LBB19_9
	mov	w13, #0
	stp	x12, x11, [sp, #8]
	strb	w13, [sp, #24]
	; InlineAsm Start
	; InlineAsm End
	add	x0, x0, #1
	cmp	x11, x1
	b.lo	LBB19_2
	b	LBB19_15
LBB19_8:
	mov	w13, #1
	stp	x12, x11, [sp, #8]
	strb	w13, [sp, #24]
	; InlineAsm Start
	; InlineAsm End
	add	x0, x0, #1
	cmp	x11, x1
	b.lo	LBB19_2
	b	LBB19_15
LBB19_9:
	cmp	x11, x1
	b.hs	LBB19_13
LBB19_10:
	ldrb	w13, [x8, x11]
	ldrb	w13, [x10, x13]
	cbnz	w13, LBB19_14
	add	x11, x11, #1
	cmp	x1, x11
	b.ne	LBB19_10
	mov	w13, #2
	mov	x11, x1
	stp	x12, x1, [sp, #8]
	strb	w13, [sp, #24]
	; InlineAsm Start
	; InlineAsm End
	add	x0, x0, #1
	cmp	x1, x1
	b.lo	LBB19_2
	b	LBB19_15
LBB19_13:
	mov	w13, #2
	stp	x12, x11, [sp, #8]
	strb	w13, [sp, #24]
	; InlineAsm Start
	; InlineAsm End
	add	x0, x0, #1
	cmp	x11, x1
	b.lo	LBB19_2
	b	LBB19_15
LBB19_14:
	mov	w13, #2
	stp	x12, x11, [sp, #8]
	strb	w13, [sp, #24]
	; InlineAsm Start
	; InlineAsm End
	add	x0, x0, #1
	cmp	x11, x1
	b.lo	LBB19_2
LBB19_15:
	add	sp, sp, #32
	.cfi_def_cfa_offset 0
	ret
LBB19_16:
	mov	x0, #0
	ret
	.loh AdrpAdd	Lloh12, Lloh13
	.cfi_endproc
